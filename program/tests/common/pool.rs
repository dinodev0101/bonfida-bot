use std::num::{NonZeroU16, NonZeroU64};

#[cfg(not(feature = "fuzz"))]
use bonfida_bot::instruction::{
    cancel_order, create, create_order, deposit, init, init_order_tracker, redeem, settle_funds,
};

#[cfg(feature = "fuzz")]
use crate::instruction::{
    cancel_order, create, create_order, deposit, init, init_order_tracker, redeem, settle_funds,
};
use rand::{distributions::Alphanumeric, Rng};
use serum_dex::{instruction::SelfTradeBehavior, matching::Side};
use solana_program::{pubkey::Pubkey, system_program, sysvar};
use solana_program_test::{ProgramTest};
use solana_sdk::signature::{Keypair, Signer};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{instruction::mint_to, state::Mint};

use super::{market::SerumMarket, utils::{Context, MintInfo, OpenOrderView, create_and_get_associated_token_address, mint_bootstrap, wrap_process_transaction}};
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
            let (_, bump) = Pubkey::find_program_address(&[&pool_seeds[..31]], &ctx.bonfidabot_program_id);
            pool_seeds[31] = bump;
            if Pubkey::create_program_address(&[&pool_seeds, &[1]], &ctx.bonfidabot_program_id).is_ok() {
                break;
            };
        }
        let mint_key = Pubkey::create_program_address(&[&pool_seeds, &[1]], &ctx.bonfidabot_program_id).unwrap();
        Self {
            seeds: pool_seeds,
            key: Pubkey::create_program_address(&[&pool_seeds], &ctx.bonfidabot_program_id).unwrap(),
            mint_key,
            mints: vec![],
            program_id: ctx.bonfidabot_program_id,
            signal_provider: Keypair::new(),
        }
    }

    pub fn add_mint(
        &mut self,
        name: Option<&str>,
        mint_info: &MintInfo
    ) {
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

    pub async fn setup(
        &self,
        ctx: &Context
    ) {
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
        )
        .unwrap();
        let mut instructions = Vec::with_capacity(self.mints.len() + 1);
        instructions.push(init_instruction);

        instructions.extend(
            self.mints
                .iter()
                .map(|m| create_associated_token_account(&ctx.test_state.payer.pubkey(), &self.key, &m.key)),
        );

        wrap_process_transaction(
            &ctx,
            instructions,
            vec![],
        )
        .await
        .unwrap();
    }

    pub async fn get_pt_account(&self, ctx: &Context, owner: &Pubkey) -> Pubkey {

        let (initialize_target_pt_account_instruction, pooltoken_target_key) =
            create_and_get_associated_token_address(
                &ctx.test_state.payer.pubkey(),
                owner,
                &self.mint_key,
            );
        wrap_process_transaction(
            &ctx,
            vec![initialize_target_pt_account_instruction],
            vec![]
        )
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
            let (create_instruction, address) =
                create_and_get_associated_token_address(&ctx.test_state.payer.pubkey(), owner_address, &m.key);
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
        wrap_process_transaction(
            ctx,
            instructions,
            vec![&ctx.mint_authority],
        )
        .await
        .unwrap();
        accounts
    }

    pub async fn create(
        &self,
        ctx: &Context,
        target_pool_token_account: &Pubkey,
        source_owner: &Keypair,
        source_asset_keys: &Vec<Pubkey>,
        deposit_amounts: Vec<u64>,
    ) {
        let create_instruction = create(
            &spl_token::id(),
            &self.program_id,
            &self.mint_key,
            &self.key,
            self.seeds,
            &self.mints.iter().map(|m| m.pool_asset_key).collect(),
            target_pool_token_account,
            &source_owner.pubkey(),
            &source_asset_keys,
            &self.signal_provider.pubkey(),
            deposit_amounts,
        )
        .unwrap();
        wrap_process_transaction(
            &ctx,
            vec![create_instruction],
            vec![&source_owner],
        )
        .await
        .unwrap();
    }

    pub async fn deposit(
        &self,
        ctx: &Context,
        amount: u64,
        pooltoken_target_key: &Pubkey,
        source_owner: &Keypair,
        source_asset_keys: &Vec<Pubkey>,
    ) {
        let deposit_instruction = deposit(
            &spl_token::id(),
            &self.program_id,
            &self.mint_key,
            &self.key,
            &self.mints.iter().map(|m| m.pool_asset_key).collect(),
            &pooltoken_target_key,
            &source_owner.pubkey(),
            &source_asset_keys,
            self.seeds,
            amount,
        )
        .unwrap();
        wrap_process_transaction(
            &ctx,
            vec![deposit_instruction],
            vec![&source_owner],
        )
        .await
        .unwrap();
    }

    pub async fn initialize_new_order(
        &self,
        ctx: &Context,
    ) -> Order {
        let (open_order, create_open_order_instruction) =
            SerumMarket::create_dex_account(&ctx, 3216).unwrap();
        let (order_tracker_key, _) = Pubkey::find_program_address(
            &[&self.seeds, &open_order.pubkey().to_bytes()],
            &self.program_id,
        );
        let init_tracker_instruction = init_order_tracker(
            &system_program::id(),
            &sysvar::rent::id(),
            &self.program_id,
            &order_tracker_key,
            &open_order.pubkey(),
            &ctx.test_state.payer.pubkey(),
            &self.key,
            self.seeds,
        )
        .unwrap();

        wrap_process_transaction(
            &ctx,
            vec![create_open_order_instruction, init_tracker_instruction],
            vec![&open_order],
        )
        .await
        .unwrap();
        Order {
            open_orders_account: open_order.pubkey(),
            order_tracker_account: order_tracker_key,
        }
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
    ) {
        let create_order_instruction = create_order(
            &self.program_id,
            &self.signal_provider.pubkey(),
            &serum_market.market_key.pubkey(),
            &self.mints[source_asset_index as usize].pool_asset_key,
            source_asset_index,
            target_asset_index,
            &order.open_orders_account,
            &order.order_tracker_account,
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
        .unwrap();
    }

    pub async fn settle(
        &self,
        ctx: &Context,
        serum_market: &SerumMarket,
        coin_asset_index: u64,
        pc_asset_index: u64,
        order: &Order,
    ) {
        let settle_instruction = settle_funds(
            &self.program_id,
            &serum_market.market_key.pubkey(),
            &order.open_orders_account,
            &order.order_tracker_account,
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
        wrap_process_transaction(
            &ctx,
            vec![settle_instruction],
            vec![],
        )
        .await
        .unwrap();
    }

    pub async fn cancel_order(
        &self,
        ctx: &Context,
        serum_market: &SerumMarket,
        order: &Order,
    ) {
        let openorder_view = OpenOrderView::get(order.open_orders_account, &ctx.test_state.banks_client).await;
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
        wrap_process_transaction(
            &ctx,
            vec![cancel_instruction],
            vec![&self.signal_provider],
        )
        .await
        .unwrap();
    }

    pub async fn redeem(
        &self,
        ctx: &Context,
        amount: u64,
        source_owner: &Keypair,
        pooltoken_target_key: &Pubkey,
        source_asset_keys: &Vec<Pubkey>,
    ) {
        let redeem_instruction = redeem(
            &spl_token::id(),
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
        wrap_process_transaction(
            &ctx,
            vec![redeem_instruction],
            vec![&source_owner],
        )
        .await
        .unwrap();
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
    pub order_tracker_account: Pubkey,
}
