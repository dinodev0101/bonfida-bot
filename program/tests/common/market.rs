use std::num::NonZeroU64;

use serum_dex::{instruction::SelfTradeBehavior, matching::{OrderType, Side}, state::gen_vault_signer_key};
use solana_program::{
    instruction::Instruction, pubkey::Pubkey, rent::Rent,
    sysvar,
};

use solana_sdk::{signature::{Keypair, Signer}, transport::TransportError};
use spl_token::instruction::mint_to;

use super::utils::{Context, OpenOrderView, create_token_account, wrap_process_transaction};

pub struct SerumMarket {
    pub market_key: Keypair,
    pub req_q_key: Keypair,
    pub event_q_key: Keypair,
    pub bids_key: Keypair,
    pub asks_key: Keypair,
    pub coin_lot_size: u64,
    pub pc_lot_size: u64,
    pub vault_signer_pk: Pubkey,
    pub vault_signer_nonce: u64,
    pub coin_fee_receiver: Pubkey,
    pub pc_fee_receiver: Pubkey,
    pub coin_vault: Pubkey,
    pub pc_vault: Pubkey,
    pub coin_mint: Pubkey,
    pub pc_mint: Pubkey,
}

impl SerumMarket {
    pub async fn initialize_market_accounts(
        ctx: &Context,
        coin_mint: &Pubkey,
        pc_mint: &Pubkey,
    ) -> Result<Self, TransportError> {
        let (market_key, create_market) =
            Self::create_dex_account(&ctx, 376)?;
        let (req_q_key, create_req_q) =
            Self::create_dex_account(&ctx, 6400)?;
        let (event_q_key, create_event_q) =
            Self::create_dex_account(&ctx, 1 << 20)?;
        let (bids_key, create_bids) =
            Self::create_dex_account(&ctx, 1 << 16)?;
        let (asks_key, create_asks) =
            Self::create_dex_account(&ctx, 1 << 16)?;
        let (vault_signer_nonce, vault_signer_pk) = {
            let mut i = 0;
            loop {
                assert!(i < 100);
                if let Ok(pk) = gen_vault_signer_key(i, &market_key.pubkey(), &ctx.serum_program_id) {
                    break (i, pk);
                }
                i += 1;
            }
        };
        let create_instructions = vec![
            create_market,
            create_req_q,
            create_event_q,
            create_bids,
            create_asks,
        ];
        let keys = vec![&market_key, &req_q_key, &event_q_key, &bids_key, &asks_key];
        wrap_process_transaction(
            &ctx,
            create_instructions,
            keys,
        )
        .await?;

        // Create Vaults
        let coin_vault = Keypair::new();
        let pc_vault = Keypair::new();
        let create_coin_vault = create_token_account(
            &ctx,
            coin_mint,
            &coin_vault,
            &vault_signer_pk,
        );
        ctx.test_state.banks_client
            .to_owned()
            .process_transaction(create_coin_vault)
            .await?;
        let create_pc_vault = create_token_account(
            &ctx,
            pc_mint,
            &pc_vault,
            &vault_signer_pk,
        );
        ctx.test_state.banks_client
            .to_owned()
            .process_transaction(create_pc_vault)
            .await?;

        // Create fee receivers
        let coin_fee_receiver = Keypair::new();
        let pc_fee_receiver = Keypair::new();
        let create_coin_fee_receiver = create_token_account(
            &ctx,
            coin_mint,
            &coin_fee_receiver,
            &Pubkey::new_unique(),
        );
        ctx.test_state.banks_client
            .to_owned()
            .process_transaction(create_coin_fee_receiver)
            .await?;
        let create_pc_fee_receiver = create_token_account(
            &ctx,
            &pc_mint,
            &pc_fee_receiver,
            &Pubkey::new_unique(),
        );
        ctx.test_state.banks_client
            .to_owned()
            .process_transaction(create_pc_fee_receiver)
            .await?;

        let init_market_instruction = serum_dex::instruction::initialize_market(
            &market_key.pubkey(),
            &ctx.serum_program_id,
            coin_mint,
            pc_mint,
            &coin_vault.pubkey(),
            &pc_vault.pubkey(),
            &bids_key.pubkey(),
            &asks_key.pubkey(),
            &req_q_key.pubkey(),
            &event_q_key.pubkey(),
            1000,
            1,
            vault_signer_nonce,
            100,
        ).unwrap();
        let serum_market = SerumMarket {
            market_key,
            req_q_key,
            event_q_key,
            bids_key,
            asks_key,
            coin_lot_size: 1000,
            pc_lot_size: 1,
            vault_signer_pk,
            vault_signer_nonce,
            coin_fee_receiver: coin_fee_receiver.pubkey(),
            pc_fee_receiver: pc_fee_receiver.pubkey(),
            coin_vault: coin_vault.pubkey(),
            pc_vault: pc_vault.pubkey(),
            coin_mint: *coin_mint,
            pc_mint: *pc_mint,
        };
        wrap_process_transaction(
            &ctx,
            vec![init_market_instruction],
            vec![],
        )
        .await?;

        Ok(serum_market)
    }

    pub fn create_dex_account(
        ctx: &Context,
        unpadded_len: usize,
    ) -> Result<(Keypair, Instruction), TransportError> {
        let len = unpadded_len + 12;
        let key = Keypair::new();
        let create_account_instr = solana_sdk::system_instruction::create_account(
            &ctx.test_state.payer.pubkey(),
            &key.pubkey(),
            Rent::default().minimum_balance(len),
            len as u64,
            &ctx.serum_program_id,
        );
        Ok((key, create_account_instr))
    }

    pub async fn match_and_crank_order(
        &self,
        ctx: &Context,
        side: Side,
        limit_price: NonZeroU64,
        max_qty: NonZeroU64,
        client_id: u64,
        self_trade_behavior: SelfTradeBehavior,
        asset_mint_authority: &Keypair,
        open_order_to_match: &Pubkey,
    ) -> Pubkey {
        // Create and mint to coin source
        let coin_source = Keypair::new();
        let coin_source_owner = Keypair::new();
        let create_coin_source = create_token_account(
            &ctx,
            &self.coin_mint,
            &coin_source,
            &coin_source_owner.pubkey(),
        );
        &ctx.test_state.banks_client
            .to_owned()
            .process_transaction(create_coin_source)
            .await
            .unwrap();
        let mint_coin_source_instruction = mint_to(
            &spl_token::id(),
            &self.coin_mint,
            &coin_source.pubkey(),
            &asset_mint_authority.pubkey(),
            &[],
            (u64::MAX as u64) >> 1,
        )
        .unwrap();
        wrap_process_transaction(
            &ctx, 
            vec![mint_coin_source_instruction],
            vec![&asset_mint_authority],
        )
        .await
        .unwrap();

        let (matching_open_order, create_matching_order_instruction) =
            SerumMarket::create_dex_account(&ctx, 3216).unwrap();
        wrap_process_transaction(
            &ctx,
            vec![create_matching_order_instruction],
            vec![&matching_open_order],
        )
        .await
        .unwrap();

        let max_native_pc_qty_including_fees = match side {
            Side::Bid => {NonZeroU64::new(max_qty.get() * self.coin_lot_size * self.pc_lot_size / limit_price.get()).unwrap()}
            Side::Ask => {NonZeroU64::new(1).unwrap()}
        };

        let matching_instruction = serum_dex::instruction::new_order(
            &self.market_key.pubkey(),
            &matching_open_order.pubkey(),
            &self.req_q_key.pubkey(),
            &self.event_q_key.pubkey(),
            &self.bids_key.pubkey(),
            &self.asks_key.pubkey(),
            &coin_source.pubkey(),
            &coin_source_owner.pubkey(),
            &self.coin_vault,
            &self.pc_vault,
            &spl_token::id(),
            &sysvar::rent::id(),
            None,
            &ctx.serum_program_id,
            match side {
                Side::Ask => Side::Bid,
                Side::Bid => Side::Ask,
            },
            limit_price,
            max_qty,
            OrderType::Limit,
            client_id,
            self_trade_behavior,
            1000,
            max_native_pc_qty_including_fees
        )
        .unwrap();
        wrap_process_transaction(
            &ctx,
            vec![matching_instruction],
            vec![&coin_source_owner],
        )
        .await
        .unwrap();

        let openorder_view = OpenOrderView::parse(
            ctx.test_state.banks_client
                .to_owned()
                .get_account(matching_open_order.pubkey())
                .await
                .unwrap()
                .unwrap()
                .data,
        );
        println!(
            "Matching Open order account before matching: {:?}",
            openorder_view
        );

        self.crank(
            ctx,
            vec![&matching_open_order.pubkey(), &open_order_to_match],
        )
        .await;

        matching_open_order.pubkey()
    }

    pub async fn crank(
        &self,
        ctx: &Context,
        open_order_accounts: Vec<&Pubkey>,
    ) {

        // Crank the Serum matching engine
        let match_instruction = serum_dex::instruction::match_orders(
            &ctx.serum_program_id,
            &self.market_key.pubkey(),
            &self.req_q_key.pubkey(),
            &self.bids_key.pubkey(),
            &self.asks_key.pubkey(),
            &self.event_q_key.pubkey(),
            &self.coin_fee_receiver,
            &self.pc_fee_receiver,
            500,
        )
        .unwrap();
        wrap_process_transaction(
            &ctx,
            vec![match_instruction],
            vec![],
        )
        .await
        .unwrap();

        let consume_instruction = serum_dex::instruction::consume_events(
            &ctx.serum_program_id,
            open_order_accounts,
            &self.market_key.pubkey(),
            &self.event_q_key.pubkey(),
            &self.coin_fee_receiver,
            &self.pc_fee_receiver,
            500,
        )
        .unwrap();
        wrap_process_transaction(
            &ctx,
            vec![consume_instruction],
            vec![],
        )
        .await
        .unwrap();
    }
}
