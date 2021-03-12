use std::num::{NonZeroU16, NonZeroU64};

#[cfg(feature = "fuzz")]
use arbitrary::{Arbitrary, Unstructured};

use serum_dex::matching::Side;
use solana_program::{instruction::InstructionError, program_pack::Pack, pubkey::Pubkey};
use solana_sdk::{
    signature::{Keypair, Signer},
    transport::TransportError,
};
use spl_token::state::Account;

use super::{
    market::SerumMarket,
    pool::{Order, TestPool},
    utils::{clone_keypair, into_transport_error, result_err_filter, get_element_from_seed, Context, MintInfo},
};

#[cfg(feature = "fuzz")]
use super::utils::arbitraryNonZeroU8;

pub struct Actor {
    pub key: Keypair,
    pub asset_accounts: Vec<Pubkey>,
    pub pool_token_balance: u64,
    pub pool_token_account: Option<Pubkey>,
    pub signal_provider: bool,
}

impl Clone for Actor {
    fn clone(&self) -> Self {
        Self {
            key: clone_keypair(&self.key),
            asset_accounts: self.asset_accounts.clone(),
            pool_token_balance: self.pool_token_balance,
            pool_token_account: self.pool_token_account,
            signal_provider: self.signal_provider,
        }
    }
}

#[cfg(feature = "fuzz")]
impl Arbitrary<'_> for Actor {
    fn arbitrary(_: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        Ok(Actor {
            key: Keypair::new(),
            asset_accounts: vec![],
            pool_token_balance: 0,
            pool_token_account: None,
            signal_provider: false,
        })
    }
}

impl Actor {
    pub fn is_bought_in(&self) -> bool {
        self.pool_token_balance == 0
    }
}

#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
#[derive(Debug)]
pub enum Intention {
    Idle,
    BuyIn(u8),
    BuyOutPartial(u8),
    BuyOut,
    Attack(u32),
}

pub enum Signal {
    Idle,
    CreateOrder {
        side: Side,
        limit_price: NonZeroU64,
        max_qty: NonZeroU16,
        cancel_after: u8,
    },
}

#[cfg(feature = "fuzz")]
impl Arbitrary<'_> for Signal {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        let result = match u.choose(&[0, 1])? {
            0 => Self::Idle,
            1 => Self::CreateOrder {
                side: *u.choose(&[Side::Ask, Side::Bid])?,
                limit_price: arbitraryNonZeroU8(u)?.into(),
                max_qty: arbitraryNonZeroU8(u)?.into(),
                cancel_after: u.arbitrary()?,
            },
            _ => {
                unreachable!()
            }
        };
        Ok(result)
    }
}
pub struct Turn {
    signal_intention: Signal,
    actor_intentions: Vec<Intention>,
}

pub struct Universe {
    cycle: u8,
    known_accounts: Vec<Pubkey>,
    active_orders: Vec<(u8, Order)>,
    pool: TestPool,
    pool_token_supply: u64,
    serum_market: Option<SerumMarket>,
    actors: Vec<Actor>,
}

pub struct Execution {
    subscribers: Vec<Actor>,
    initial_deposit_amounts: Vec<u8>,
    turns: Vec<Turn>,
}

#[cfg(feature = "fuzz")]
impl Arbitrary<'_> for Execution {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        let number_of_subscribers: u8 = u.arbitrary::<u8>()? >> 4;
        let mut subscribers = Vec::with_capacity(number_of_subscribers as usize);
        for _ in 0..number_of_subscribers {
            subscribers.push(u.arbitrary()?)
        }
        let mut turns = Vec::with_capacity(100);
        for _ in 0..20 {
            let mut actor_intentions = Vec::with_capacity((number_of_subscribers as usize) + 1);
            for _ in 0..((number_of_subscribers as u16) + 1) {
                actor_intentions.push(u.arbitrary()?)
            }
            turns.push(Turn {
                signal_intention: u.arbitrary()?,
                actor_intentions,
            })
        }
        let mut initial_deposit_amounts = Vec::with_capacity(4);
        for _ in 0..4 {
            initial_deposit_amounts.push(u.arbitrary()?);
        }
        initial_deposit_amounts[0] = 10.max(initial_deposit_amounts[0]);
        println!(
            "Initial deposit amounts length : {:?}",
            initial_deposit_amounts.len()
        );
        Ok(Execution {
            subscribers,
            initial_deposit_amounts,
            turns,
        })
    }
}

impl Execution {
    pub async fn run(&self, ctx: &mut Context, mints: &Vec<MintInfo>) {
        let mut universe = Universe::new(&ctx, mints);

        for a in &self.subscribers {
            universe.add_actor(a.clone())
        }

        println!("=========== Universe init ===========");

        result_err_filter(
            universe
                .init(
                    ctx,
                    self.initial_deposit_amounts
                        .iter()
                        .map(|a| u64::from(*a) * 100_000)
                        .collect(),
                )
                .await,
        )
        .unwrap();
        println!("=========== Simulation Turns ===========");
        for turn in &self.turns {
            result_err_filter(universe.consume_turn(ctx, turn).await).unwrap();
            if universe.pool_token_supply == 0 {
                break;
            }
        }
    }
}

impl Universe {
    pub fn new(ctx: &Context, mints: &Vec<MintInfo>) -> Self {
        let mut pool = TestPool::new(&ctx);
        for mint_info in mints {
            pool.add_mint(None, mint_info);
        }
        let known_accounts = pool
            .mints
            .iter()
            .map(|m| m.pool_asset_key)
            .collect::<Vec<_>>();

        let signal_provider = Actor {
            key: clone_keypair(&pool.signal_provider),
            asset_accounts: vec![],
            signal_provider: true,
            pool_token_balance: 0,
            pool_token_account: None,
        };

        Self {
            cycle: 0,
            known_accounts,
            pool,
            pool_token_supply: 0,
            actors: vec![signal_provider],
            active_orders: vec![],
            serum_market: None,
        }
    }

    pub fn add_actor(&mut self, actor: Actor) {
        self.actors.push(actor)
    }

    pub async fn init(
        &mut self,
        ctx: &mut Context,
        deposit_amounts: Vec<u64>,
    ) -> Result<(), TransportError> {
        self.serum_market = Some(
            SerumMarket::initialize_market_accounts(
                ctx,
                &self.pool.mints[2].key,
                &self.pool.mints[1].key,
            )
            .await?,
        );
        self.pool.setup(&ctx).await;
        for actor in &mut self.actors {
            actor.asset_accounts = self
                .pool
                .get_funded_token_accounts(ctx, &actor.key.pubkey())
                .await;
            self.known_accounts.extend(actor.asset_accounts.iter());
            actor.pool_token_account =
                Some(self.pool.get_pt_account(ctx, &actor.key.pubkey()).await);
            self.known_accounts.push(actor.pool_token_account.unwrap());
        }
        self.pool
            .create(
                &ctx,
                self.actors[0].pool_token_account.as_ref().unwrap(),
                &self.actors[0].key,
                &self.actors[0].asset_accounts,
                deposit_amounts,
                &self.serum_market.as_ref().unwrap().market_key.pubkey(),
                604800,
                15,
            )
            .await?;
        self.pool_token_supply = 1_000_000;
        self.actors[0].pool_token_balance = 1_000_000;
        Ok(())
    }

    pub async fn consume_turn(&mut self, ctx: &mut Context, turn: &Turn) -> Result<(), TransportError> {
        if self.serum_market.is_none() {
            return Err(into_transport_error(InstructionError::InvalidArgument));
        }

        if let Signal::CreateOrder {
            side,
            limit_price,
            max_qty,
            cancel_after,
        } = turn.signal_intention
        {
            let order = self.pool.initialize_new_order(ctx).await?;
            let (source_asset_index, target_asset_index) = match side {
                Side::Bid => (1, 2),
                Side::Ask => (2, 1),
            };
            let order_result = self
                .pool
                .create_new_order(
                    ctx,
                    self.serum_market.as_ref().unwrap(),
                    source_asset_index,
                    target_asset_index,
                    &order,
                    side,
                    limit_price,
                    max_qty,
                )
                .await;
            self.known_accounts.push(order.open_orders_account);
            if order_result.is_ok() {
                self.active_orders.push((cancel_after + self.cycle, order));
            }
            order_result?;
        }
        let mut active_orders = vec![];
        for (cancel_after, order) in &self.active_orders {
            if *cancel_after >= self.cycle {
                self.pool
                    .cancel_order(ctx, self.serum_market.as_ref().unwrap(), order)
                    .await?;
            } else {
                active_orders.push((*cancel_after, order.clone()))
            }
        }
        self.active_orders = active_orders;
        for i in 0..self.actors.len() {
            let actor = &mut self.actors[i];
            match turn.actor_intentions[i] {
                Intention::Idle => {}
                Intention::BuyIn(amount) => {
                    println!("Buying in");
                    let desired_amount = (amount as u64) * 100_000;
                    let result = self
                        .pool
                        .deposit(
                            ctx,
                            desired_amount,
                            actor.pool_token_account.as_ref().unwrap(),
                            &actor.key,
                            &actor.asset_accounts,
                        )
                        .await;
                    if result.is_ok() {
                        let existing_balance = actor.pool_token_balance;
                        actor.pool_token_balance = Account::unpack(
                            &ctx.test_state
                                .banks_client
                                .to_owned()
                                .get_account(actor.pool_token_account.unwrap())
                                .await
                                .unwrap()
                                .unwrap()
                                .data,
                        )
                        .unwrap()
                        .amount;
                        self.pool_token_supply =
                            self.pool_token_supply + actor.pool_token_balance - existing_balance;
                    }
                    result?
                }
                Intention::BuyOutPartial(amount) => {
                    println!("Buying out partially");
                    let actual_amount = actor.pool_token_balance.min((amount as u64) * 100_000);
                    if actual_amount != 0 {
                        let result = self
                            .pool
                            .redeem(
                                ctx,
                                actual_amount,
                                &actor.key,
                                actor.pool_token_account.as_ref().unwrap(),
                                &actor.asset_accounts,
                            )
                            .await;
                        if result.is_ok() {
                            actor.pool_token_balance = actor.pool_token_balance - actual_amount;
                            self.pool_token_supply = self.pool_token_supply - actual_amount;
                        }
                        result?;
                    }
                }
                Intention::BuyOut => {
                    println!("Buying out");
                    if actor.pool_token_balance != 0 {
                        let result = self
                            .pool
                            .redeem(
                                ctx,
                                actor.pool_token_balance,
                                &actor.key,
                                actor.pool_token_account.as_ref().unwrap(),
                                &actor.asset_accounts,
                            )
                            .await;
                        if result.is_ok() {
                            self.pool_token_supply =
                                self.pool_token_supply - actor.pool_token_balance;
                            actor.pool_token_balance = 0;
                        }
                    }
                }
                Intention::Attack(seed) => {
                    println!("Attacking");
                    let instruction_tag = seed >> 29;
                    match instruction_tag {
                        0 => {
                            let target_pool_token_account =
                                get_element_from_seed(&self.known_accounts, (seed & 0x3f) as u8);
                            let asset_accounts = &vec![
                                *get_element_from_seed(
                                    &self.known_accounts,
                                    ((seed >> 4) & 0x3f) as u8,
                                ),
                                *get_element_from_seed(
                                    &self.known_accounts,
                                    ((seed >> 8) & 0x3f) as u8,
                                ),
                            ];
                            let deposit_amounts = vec![
                                (((seed >> 12) & 0x3f) as u64) * 100_000,
                                (((seed >> 16) & 0x3f) as u64) * 100_000,
                            ];
                            let result = self
                                .pool
                                .create(
                                    ctx,
                                    target_pool_token_account,
                                    &actor.key,
                                    asset_accounts,
                                    deposit_amounts,
                                    &self.serum_market.as_ref().unwrap().market_key.pubkey(),
                                    700_000,
                                    15,
                                )
                                .await;
                            result_err_filter(result)?;
                        }
                        1 => {
                            let target_pool_token_account =
                                get_element_from_seed(&self.known_accounts, (seed & 0x3f) as u8);
                            let amount = (((seed >> 4) & 0x3f) as u64) * 100_000;
                            let asset_accounts = &vec![
                                *get_element_from_seed(
                                    &self.known_accounts,
                                    ((seed >> 8) & 0x3f) as u8,
                                ),
                                *get_element_from_seed(
                                    &self.known_accounts,
                                    ((seed >> 12) & 0x3f) as u8,
                                ),
                            ];

                            let result = self.pool.deposit(
                                ctx,
                                amount,
                                target_pool_token_account,
                                &actor.key,
                                asset_accounts,
                            ).await;
                            result_err_filter(result)?;
                        }
                        2 => {
                            let order = Order {
                                open_orders_account: *get_element_from_seed(&self.known_accounts, (seed & 0x3f) as u8)
                            };
                            let side = match seed >> 31 {
                                0 => {Side::Ask}
                                1 => {Side::Bid}
                                _ => {unreachable!()}
                            };
                            let result = self.pool.create_new_order(
                                ctx, 
                                self.serum_market.as_ref().unwrap(), 
                                ((seed >> 16) & 0x3f) as u64 % (self.pool.mints.len() as u64), 
                                ((seed >> 20) & 0x3f) as u64 % (self.pool.mints.len() as u64), 
                                &order, 
                                side, 
                                NonZeroU64::new((((seed >> 24) & 0x3f) << 4) as u64 + 1).unwrap(), 
                                NonZeroU16::new((((seed >> 28) & 0x3f) << 4) as u16 + 1).unwrap()
                            ).await;
                            result_err_filter(result)?;
                        }
                        3 => {
                            let order = Order {
                                open_orders_account: *get_element_from_seed(&self.known_accounts, (seed & 0x3f) as u8)
                            };
                            let result = self.pool.settle(
                                ctx,
                                self.serum_market.as_ref().unwrap(),
                                ((seed >> 4) & 0x3f) as u64 % (self.pool.mints.len() as u64),
                                ((seed >> 8) & 0x3f) as u64 % (self.pool.mints.len() as u64),
                                &order
                            ).await;
                            result_err_filter(result)?;
                        }
                        4 => {
                            let order = Order {
                                open_orders_account: *get_element_from_seed(&self.known_accounts, (seed & 0x3f) as u8)
                            };
                            let result = self.pool.cancel_order(
                                ctx, 
                                self.serum_market.as_ref().unwrap(), 
                                &order
                            ).await;
                            result_err_filter(result)?;
                        }
                        5 => {
                            let target_pool_token_account =
                                get_element_from_seed(&self.known_accounts, (seed & 0x3f) as u8);
                            let asset_accounts = &vec![
                                *get_element_from_seed(
                                    &self.known_accounts,
                                    ((seed >> 4) & 0x3f) as u8,
                                ),
                                *get_element_from_seed(
                                    &self.known_accounts,
                                    ((seed >> 8) & 0x3f) as u8,
                                ),
                            ];
                            let result = self.pool.redeem(
                                ctx, 
                                (((seed >> 12) & 0x3f) << 4) as u64, 
                                &actor.key, 
                                target_pool_token_account, 
                                asset_accounts
                            ).await;
                            result_err_filter(result)?;
                        }
                        6 => {
                            let result = self.pool.collect_fees(ctx).await;
                            result_err_filter(result)?;
                        }
                        7 => {}
                        _ => {
                            unreachable!()
                        }
                    }
                }
            }
            if self.pool_token_supply == 0 {
                println!("Pool is empty and has been deleted");
                break;
            }
        }
        if self.active_orders.len() != 0 {
            self.serum_market
                .as_ref()
                .unwrap()
                .crank(
                    ctx,
                    self.active_orders
                        .iter()
                        .map(|o| &o.1.open_orders_account)
                        .collect(),
                )
                .await;
        }

        Ok(())
    }
}
