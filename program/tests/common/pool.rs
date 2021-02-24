use std::{num::{NonZeroU16, NonZeroU64}, str::FromStr};

#[cfg(not(feature = "fuzz"))]
use bonfida_bot::instruction::{
    cancel_order, create, create_order, deposit, init, redeem, settle_funds,
};
use bonfida_bot::state::{BONFIDA_BNB, BONFIDA_FEE};

#[cfg(feature = "fuzz")]
use crate::instruction::{cancel_order, create, create_order, deposit, init, redeem, settle_funds};
use rand::{distributions::Alphanumeric, Rng};
use serum_dex::{instruction::SelfTradeBehavior, matching::Side};
use solana_program::{pubkey::Pubkey, system_program, sysvar};
use solana_program_test::ProgramTest;
use solana_sdk::{
    signature::{Keypair, Signer},
    transport::TransportError,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{instruction::mint_to, state::Mint};

use super::{
    market::SerumMarket,
    utils::{
        create_and_get_associated_token_address, mint_bootstrap, wrap_process_transaction, Context,
        MintInfo, OpenOrderView,
    },
};
pub struct TestPool {
    pub seeds: [u8; 32],
    pub mint_key: Pubkey,
    pub key: Pubkey,
    pub signal_provider: Keypair,
    pub mints: Vec<TestMint>,
    program_id: Pubkey,
}

impl TestPool {
    pub fn new(ctx: &Context) -> Self {
        let mut pool_seeds;
        loop {
            pool_seeds = rand::thread_rng().gen::<[u8; 32]>();
            let (_, bump) =
                Pubkey::find_program_address(&[&pool_seeds[..31]], &ctx.bonfidabot_program_id);
            pool_seeds[31] = bump;
            if Pubkey::create_program_address(&[&pool_seeds, &[1]], &ctx.bonfidabot_program_id)
                .is_ok()
            {
                break;
            };
        }
        let mint_key =
            Pubkey::create_program_address(&[&pool_seeds, &[1]], &ctx.bonfidabot_program_id)
                .unwrap();
        Self {
            seeds: pool_seeds,
            key: Pubkey::create_program_address(&[&pool_seeds], &ctx.bonfidabot_program_id)
                .unwrap(),
            mint_key,
            mints: vec![],
            program_id: ctx.bonfidabot_program_id,
            signal_provider: Keypair::new(),
        }
    }

    pub fn add_mint(&mut self, name: Option<&str>, mint_info: &MintInfo) {
        let name = name.map(String::from).unwrap_or_else(|| {
            rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect()
        });

        let pool_asset_key = get_associated_token_address(&self.key, &mint_info.0);

        self.mints.push(TestMint {
            name,
            key: mint_info.0,
            mint_params: mint_info.1,
            pool_asset_key,
        });
    }

    pub async fn setup(&self, ctx: &Context) {
        // Initialize the pool
        let init_instruction = init(
            &spl_token::id(),
            &system_program::id(),
            &sysvar::rent::id(),
            &self.program_id,
            &self.mint_key,
            &ctx.test_state.payer.pubkey(),
            &self.key,
            self.seeds,
            100,
            1,
        )
        .unwrap();
        let mut instructions = Vec::with_capacity(self.mints.len() + 1);
        instructions.push(init_instruction);

        instructions.extend(self.mints.iter().map(|m| {
            create_associated_token_account(&ctx.test_state.payer.pubkey(), &self.key, &m.key)
        }));

        wrap_process_transaction(&ctx, instructions, vec![])
            .await
            .unwrap();
        
        // Initialize fee accounts
        self.get_pt_account(ctx, &Pubkey::from_str(BONFIDA_FEE).unwrap()).await;
        self.get_pt_account(ctx, &Pubkey::from_str(BONFIDA_BNB).unwrap()).await;
        
    }

    pub async fn get_pt_account(&self, ctx: &Context, owner: &Pubkey) -> Pubkey {
        let (initialize_target_pt_account_instruction, pooltoken_target_key) =
            create_and_get_associated_token_address(
                &ctx.test_state.payer.pubkey(),
                owner,
                &self.mint_key,
            );
        wrap_process_transaction(&ctx, vec![initialize_target_pt_account_instruction], vec![])
            .await
            .unwrap();
        pooltoken_target_key
    }

    pub async fn get_funded_token_accounts(
        &self,
        ctx: &Context,
        owner_address: &Pubkey,
    ) -> Vec<Pubkey> {
        let mut accounts = Vec::with_capacity(self.mints.len());
        let mut instructions = Vec::with_capacity(self.mints.len());
        for m in self.mints.iter() {
            let (create_instruction, address) = create_and_get_associated_token_address(
                &ctx.test_state.payer.pubkey(),
                owner_address,
                &m.key,
            );
            let mint_to_instruction = mint_to(
                &spl_token::id(),
                &m.key,
                &address,
                &ctx.mint_authority.pubkey(),
                &[],
                1 << 25,
            )
            .unwrap();
            accounts.push(address);
            instructions.push(create_instruction);
            instructions.push(mint_to_instruction);
        }
        wrap_process_transaction(ctx, instructions, vec![&ctx.mint_authority])
            .await
            .unwrap();
        accounts
    }

    pub fn get_signal_provider_accounts(
        &self
    ) -> Vec<Pubkey> {
        let mut result = Vec::with_capacity(self.mints.len());;
        for m in self.mints.iter() {
            result.push(get_associated_token_address(&self.signal_provider.pubkey(), &m.key));
        };
        result
    }

    pub async fn create(
        &self,
        ctx: &Context,
        target_pool_token_account: &Pubkey,
        source_owner: &Keypair,
        source_asset_keys: &Vec<Pubkey>,
        deposit_amounts: Vec<u64>,
        market: &Pubkey,
        fee_collection_period: u64,
        fee_ratio: u16
    ) -> Result<(), TransportError> {
        println!("Deposit amounts length {:#?}", deposit_amounts.len());
        let create_instruction = create(
            &spl_token::id(),
            &sysvar::clock::id(),
            &self.program_id,
            &self.mint_key,
            &self.key,
            self.seeds,
            &self.mints.iter().map(|m| m.pool_asset_key).collect(),
            target_pool_token_account,
            &source_owner.pubkey(),
            &source_asset_keys,
            &ctx.serum_program_id,
            &self.signal_provider.pubkey(),
            fee_collection_period,
            fee_ratio,
            deposit_amounts,
            vec![market.clone()],
        )
        .unwrap();
        wrap_process_transaction(&ctx, vec![create_instruction], vec![&source_owner]).await
    }

    pub async fn deposit(
        &self,
        ctx: &Context,
        amount: u64,
        pooltoken_target_key: &Pubkey,
        source_owner: &Keypair,
        source_asset_keys: &Vec<Pubkey>,
    ) -> Result<(), TransportError> {
        let deposit_instruction = deposit(
            &spl_token::id(),
            &self.program_id,
            &self.mint_key,
            &self.key,
            &self.mints.iter().map(|m| m.pool_asset_key).collect(),
            &pooltoken_target_key,
            &get_associated_token_address(&self.signal_provider.pubkey(), &self.mint_key),
            &source_owner.pubkey(),
            &source_asset_keys,
            &self.get_signal_provider_accounts(),
            self.seeds,
            amount,
        )
        .unwrap();
        wrap_process_transaction(&ctx, vec![deposit_instruction], vec![&source_owner]).await
    }

    pub async fn initialize_new_order(&self, ctx: &Context) -> Result<Order, TransportError> {
        let (open_order, create_open_order_instruction) =
            SerumMarket::create_dex_account(&ctx, 3216).unwrap();

        wrap_process_transaction(&ctx, vec![create_open_order_instruction], vec![&open_order])
            .await?;
        Ok(Order {
            open_orders_account: open_order.pubkey(),
        })
    }

    pub async fn create_new_order(
        &self,
        ctx: &Context,
        serum_market: &SerumMarket,
        source_asset_index: u64,
        target_asset_index: u64,
        order: &Order,
        side: Side,
        limit_price: NonZeroU64,
        max_qty: NonZeroU16,
    ) -> Result<(), TransportError> {
        let create_order_instruction = create_order(
            &self.program_id,
            &self.signal_provider.pubkey(),
            &serum_market.market_key.pubkey(),
            &self.mints[source_asset_index as usize].pool_asset_key,
            source_asset_index,
            target_asset_index,
            &order.open_orders_account,
            &serum_market.req_q_key.pubkey(),
            &self.key,
            &serum_market.coin_vault,
            &serum_market.pc_vault,
            &spl_token::id(),
            &ctx.serum_program_id,
            &sysvar::rent::id(),
            None,
            self.seeds,
            side,
            limit_price,
            0,
            serum_market.coin_lot_size,
            serum_market.pc_lot_size,
            &self.mints[target_asset_index as usize].key,
            max_qty,
            serum_dex::matching::OrderType::Limit,
            0,
            SelfTradeBehavior::DecrementTake,
        )
        .unwrap();
        wrap_process_transaction(
            &ctx,
            vec![create_order_instruction],
            vec![&self.signal_provider],
        )
        .await
    }

    pub async fn settle(
        &self,
        ctx: &Context,
        serum_market: &SerumMarket,
        coin_asset_index: u64,
        pc_asset_index: u64,
        order: &Order,
    ) -> Result<(), TransportError> {
        let settle_instruction = settle_funds(
            &self.program_id,
            &serum_market.market_key.pubkey(),
            &order.open_orders_account,
            &self.key,
            &self.mint_key,
            &serum_market.coin_vault,
            &serum_market.pc_vault,
            &self.mints[coin_asset_index as usize].pool_asset_key,
            &self.mints[pc_asset_index as usize].pool_asset_key,
            &serum_market.vault_signer_pk,
            &spl_token::id(),
            &ctx.serum_program_id,
            None,
            self.seeds,
            pc_asset_index,
            coin_asset_index,
        )
        .unwrap();
        wrap_process_transaction(&ctx, vec![settle_instruction], vec![])
            .await
    }

    pub async fn cancel_order(&self, ctx: &Context, serum_market: &SerumMarket, order: &Order) -> Result<(), TransportError> {
        let openorder_view =
            OpenOrderView::get(order.open_orders_account, &ctx.test_state.banks_client).await;
        let cancel_instruction = cancel_order(
            &self.program_id,
            &self.signal_provider.pubkey(),
            &serum_market.market_key.pubkey(),
            &order.open_orders_account,
            &serum_market.req_q_key.pubkey(),
            &self.key,
            &ctx.serum_program_id,
            self.seeds,
            Side::Bid,
            openorder_view.orders[0],
        )
        .unwrap();
        wrap_process_transaction(&ctx, vec![cancel_instruction], vec![&self.signal_provider])
            .await
    }

    pub async fn redeem(
        &self,
        ctx: &Context,
        amount: u64,
        source_owner: &Keypair,
        pooltoken_target_key: &Pubkey,
        source_asset_keys: &Vec<Pubkey>,
    ) -> Result<(), TransportError> {
        let redeem_instruction = redeem(
            &spl_token::id(),
            &sysvar::clock::id(),
            &self.program_id,
            &self.mint_key,
            &self.key,
            &self.mints.iter().map(|m| m.pool_asset_key).collect(),
            &source_owner.pubkey(),
            &pooltoken_target_key,
            &source_asset_keys,
            self.seeds,
            amount,
        )
        .unwrap();
        wrap_process_transaction(&ctx, vec![redeem_instruction], vec![&source_owner])
            .await
    }

    pub async fn collect_fees(
        &self,
        ctx: &Context,
    ) {
    }
}

pub struct TestMint {
    pub name: String,
    pub key: Pubkey,
    pub mint_params: Mint,
    pub pool_asset_key: Pubkey,
}
#[derive(Clone)]
pub struct Order {
    pub open_orders_account: Pubkey,
}
