use std::num::{NonZeroU16, NonZeroU64, NonZeroU8};

#[cfg(feature = "fuzz")]
use arbitrary::{Arbitrary, Unstructured};

use serum_dex::matching::Side;
use solana_program::{
    entrypoint::ProgramResult, hash::Hash, program_error::ProgramError, pubkey::Pubkey,
};
use solana_program_test::{BanksClient, ProgramTest};
use solana_sdk::signature::{Keypair, Signer};
use spl_associated_token_account::get_associated_token_address;


use super::{market::SerumMarket, pool::{Order, TestPool}, utils::{Context, MintInfo, clone_keypair}};

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
impl Arbitrary for Actor {
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
pub enum Intention {
    Idle,
    BuyIn(u8),
    BuyOutPartial(u8),
    BuyOut,
    Attack,
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
impl Arbitrary for Signal {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        let result = match u.choose(&[0, 1])? {
            0 => {Self::Idle}
            1 => {Self::CreateOrder{
                side: *u.choose(&[Side::Ask, Side::Bid])?,
                limit_price: arbitraryNonZeroU8(u)?.into(),
                max_qty: arbitraryNonZeroU8(u)?.into(),
                cancel_after: u.arbitrary()?
            }}
            _ => {unreachable!()}
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
    serum_market: Option<SerumMarket>,
    actors: Vec<Actor>,
}

pub struct Execution {
    subscribers: Vec<Actor>,
    initial_deposit_amounts: Vec<u8>,
    turns: Vec<Turn>
}

#[cfg(feature = "fuzz")]
impl Arbitrary for Execution {
    fn arbitrary(u: &mut Unstructured<'_>) -> arbitrary::Result<Self> {
        let number_of_subscribers:u8 = u.arbitrary()?;
        let mut subscribers = Vec::with_capacity(number_of_subscribers as usize);
        for _ in 0..number_of_subscribers {
            subscribers.push(u.arbitrary()?)
        }
        let mut turns = Vec::with_capacity(100);
        for _ in 0..100 {
            let mut actor_intentions = Vec::with_capacity((number_of_subscribers as usize) + 1);
            for _ in 0..number_of_subscribers {
                actor_intentions.push(u.arbitrary()?)
            }
            turns.push(Turn {
                signal_intention: u.arbitrary()?,
                actor_intentions
            })
        }
        Ok(Execution {
            subscribers,
            initial_deposit_amounts: u.arbitrary()?,
            turns
        })
    }
}

impl Execution {

    pub async fn run(&self, ctx: &mut Context, mints: &Vec<MintInfo>) {
        let mut universe = Universe::new(&ctx, mints);

        for a in &self.subscribers {
            universe.add_actor(a.clone())
        }
        universe.init(ctx, self.initial_deposit_amounts.iter().map(|a| u64::from(*a) * 100_000).collect()).await.unwrap();
        for turn in &self.turns {
            universe.consume_turn(ctx, turn).await.unwrap();
            ctx.refresh_blockhash().await;
        }
    }
}

impl Universe {
    pub fn new(
        ctx: &Context,
        mints: &Vec<MintInfo>
    ) -> Self {
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
            actors: vec![signal_provider],
            active_orders: vec![],
            serum_market: None,
        }
    }

    pub fn add_actor(&mut self, actor: Actor) {
        self.actors.push(actor)
    }

    pub async fn init(&mut self, ctx: &mut Context, deposit_amounts: Vec<u64>) -> ProgramResult {
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
            actor.pool_token_account = Some(self.pool.get_pt_account(ctx, &actor.key.pubkey()).await);
        }
        self.pool
            .create(
                &ctx,
                self.actors[0].pool_token_account.as_ref().unwrap(),
                &self.actors[0].key,
                &self.actors[0].asset_accounts,
                deposit_amounts,
            )
            .await;
        Ok(())
    }

    pub async fn consume_turn(&mut self, ctx: &Context, turn: &Turn) -> ProgramResult {
        if self.serum_market.is_none() {
            return Err(ProgramError::InvalidArgument);
        }

        if let Signal::CreateOrder {
            side,
            limit_price,
            max_qty,
            cancel_after,
        } = turn.signal_intention
        {
            let order = self.pool.initialize_new_order(ctx).await;
            let (source_asset_index, target_asset_index) = match side {
                Side::Bid => (1, 2),
                Side::Ask => (2, 1),
            };
            self.pool
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
            self.active_orders
                .push((cancel_after + self.cycle, order));
        }
        let mut active_orders = vec![];
        for (cancel_after, order) in &self.active_orders {
            if *cancel_after >= self.cycle {
                self.pool
                    .cancel_order(ctx, self.serum_market.as_ref().unwrap(), order)
                    .await
            } else {
                active_orders.push((*cancel_after, order.clone()))
            }
        }
        for i in 0..self.actors.len() {
            let actor = &mut self.actors[i];
            match turn.actor_intentions[i - 1] {
                Intention::Idle => {}
                Intention::BuyIn(amount) => {
                    self.pool
                        .deposit(
                            ctx,
                            (amount as u64) * 100_000,
                            actor.pool_token_account.as_ref().unwrap(),
                            &actor.key,
                            &actor.asset_accounts,
                        )
                        .await;
                    actor.pool_token_balance = actor.pool_token_balance + (amount as u64) * 100_000;
                }
                Intention::BuyOutPartial(amount) => {
                    self.pool
                        .redeem(
                            ctx,
                            (amount as u64) * 100_000,
                            &actor.key,
                            actor.pool_token_account.as_ref().unwrap(),
                            &actor.asset_accounts,
                        )
                        .await;
                    actor.pool_token_balance = actor.pool_token_balance + (amount as u64) * 100_000;
                }
                Intention::BuyOut => {
                    self.pool
                        .redeem(
                            ctx,
                            actor.pool_token_balance,
                            &actor.key,
                            actor.pool_token_account.as_ref().unwrap(),
                            &actor.asset_accounts,
                        )
                        .await;
                        actor.pool_token_balance = 0;
                }
                Intention::Attack => {}
            }
        }
        Ok(())
    }
}
