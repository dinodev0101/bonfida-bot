use std::{convert::TryInto, num::NonZeroU64};

use bonfida_bot::state::{unpack_assets, PoolHeader};
use serum_dex::{
    instruction::SelfTradeBehavior,
    matching::{OrderType, Side},
    state::gen_vault_signer_key,
};
use solana_program::{
    hash::Hash, instruction::Instruction, program_error::ProgramError, program_pack::Pack,
    pubkey::Pubkey, rent::Rent, system_instruction, sysvar,
};
use solana_program_test::{BanksClient, ProgramTestBanksClientExt};
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
    transport::TransportError,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::instruction::{initialize_account, initialize_mint, mint_to};

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
        serum_program_id: &Pubkey,
        payer: &Keypair,
        coin_mint: &Keypair,
        pc_mint: &Keypair,
        recent_blockhash: Hash,
        banks_client: &BanksClient,
    ) -> Result<Self, ProgramError> {
        let (market_key, create_market) =
            Self::create_dex_account(serum_program_id, &payer.pubkey(), 376)?;
        let (req_q_key, create_req_q) =
            Self::create_dex_account(serum_program_id, &payer.pubkey(), 640)?;
        let (event_q_key, create_event_q) =
            Self::create_dex_account(serum_program_id, &payer.pubkey(), 1 << 20)?;
        let (bids_key, create_bids) =
            Self::create_dex_account(serum_program_id, &payer.pubkey(), 1 << 16)?;
        let (asks_key, create_asks) =
            Self::create_dex_account(serum_program_id, &payer.pubkey(), 1 << 16)?;
        let (vault_signer_nonce, vault_signer_pk) = {
            let mut i = 0;
            loop {
                assert!(i < 100);
                if let Ok(pk) = gen_vault_signer_key(i, &market_key.pubkey(), serum_program_id) {
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
            create_instructions,
            &payer,
            keys,
            &recent_blockhash,
            &banks_client,
        )
        .await
        .unwrap();

        // Create Vaults
        let coin_vault = Keypair::new();
        let pc_vault = Keypair::new();
        let create_coin_vault = create_token_account(
            &payer,
            &coin_mint.pubkey(),
            recent_blockhash,
            &coin_vault,
            &vault_signer_pk,
        );
        banks_client
            .to_owned()
            .process_transaction(create_coin_vault)
            .await
            .unwrap();
        let create_pc_vault = create_token_account(
            &payer,
            &pc_mint.pubkey(),
            recent_blockhash,
            &pc_vault,
            &vault_signer_pk,
        );
        banks_client
            .to_owned()
            .process_transaction(create_pc_vault)
            .await
            .unwrap();

        // Create fee receivers
        let coin_fee_receiver = Keypair::new();
        let pc_fee_receiver = Keypair::new();
        let create_coin_fee_receiver = create_token_account(
            &payer,
            &coin_mint.pubkey(),
            recent_blockhash,
            &coin_fee_receiver,
            &Pubkey::new_unique(),
        );
        banks_client
            .to_owned()
            .process_transaction(create_coin_fee_receiver)
            .await
            .unwrap();
        let create_pc_fee_receiver = create_token_account(
            &payer,
            &pc_mint.pubkey(),
            recent_blockhash,
            &pc_fee_receiver,
            &Pubkey::new_unique(),
        );
        banks_client
            .to_owned()
            .process_transaction(create_pc_fee_receiver)
            .await
            .unwrap();

        let init_market_instruction = serum_dex::instruction::initialize_market(
            &market_key.pubkey(),
            serum_program_id,
            &coin_mint.pubkey(),
            &pc_mint.pubkey(),
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
        )?;
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
            coin_mint: coin_mint.pubkey(),
            pc_mint: pc_mint.pubkey(),
        };
        wrap_process_transaction(
            vec![init_market_instruction],
            &payer,
            vec![],
            &recent_blockhash,
            &banks_client,
        )
        .await
        .unwrap();

        Ok(serum_market)
    }

    pub fn create_dex_account(
        serum_program_id: &Pubkey,
        payer: &Pubkey,
        unpadded_len: usize,
    ) -> Result<(Keypair, Instruction), ProgramError> {
        let len = unpadded_len + 12;
        let key = Keypair::new();
        let create_account_instr = solana_sdk::system_instruction::create_account(
            payer,
            &key.pubkey(),
            Rent::default().minimum_balance(len),
            len as u64,
            serum_program_id,
        );
        Ok((key, create_account_instr))
    }

    pub async fn match_and_crank_order(
        &self,
        serum_program_id: &Pubkey,
        payer: &Keypair,
        recent_blockhash: Hash,
        banks_client: &BanksClient,
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
            &payer,
            &self.coin_mint,
            recent_blockhash,
            &coin_source,
            &coin_source_owner.pubkey(),
        );
        banks_client
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
            vec![mint_coin_source_instruction],
            &payer,
            vec![&asset_mint_authority],
            &recent_blockhash,
            &banks_client,
        )
        .await
        .unwrap();

        let (matching_open_order, create_matching_order_tracker_instruction) =
            SerumMarket::create_dex_account(&serum_program_id, &payer.pubkey(), 3216).unwrap();
        wrap_process_transaction(
            vec![create_matching_order_tracker_instruction],
            &payer,
            vec![&matching_open_order],
            &recent_blockhash,
            &banks_client,
        )
        .await
        .unwrap();

        let matching_instruction = serum_dex::instruction::new_order(
            &self.market_key.pubkey(),
            &matching_open_order.pubkey(),
            &self.req_q_key.pubkey(),
            &coin_source.pubkey(),
            &coin_source_owner.pubkey(),
            &self.coin_vault,
            &self.pc_vault,
            &spl_token::id(),
            &sysvar::rent::id(),
            None,
            &serum_program_id,
            match side {
                Side::Ask => Side::Bid,
                Side::Bid => Side::Ask,
            },
            limit_price,
            max_qty,
            OrderType::Limit,
            client_id,
            self_trade_behavior,
        )
        .unwrap();
        wrap_process_transaction(
            vec![matching_instruction],
            &payer,
            vec![&coin_source_owner],
            &recent_blockhash,
            banks_client,
        )
        .await
        .unwrap();

        let openorder_view = OpenOrderView::parse(
            banks_client
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
            serum_program_id,
            &recent_blockhash,
            payer,
            banks_client,
            vec![&matching_open_order.pubkey(), &open_order_to_match],
        )
        .await;

        matching_open_order.pubkey()
    }

    pub async fn crank(
        &self,
        serum_program_id: &Pubkey,
        recent_blockhash: &Hash,
        payer: &Keypair,
        banks_client: &BanksClient,
        open_order_accounts: Vec<&Pubkey>,
    ) {
        println!("CHECKEEsss");

        // Crank the Serum matching engine
        let match_instruction = serum_dex::instruction::match_orders(
            serum_program_id,
            &self.market_key.pubkey(),
            &self.req_q_key.pubkey(),
            &self.bids_key.pubkey(),
            &self.asks_key.pubkey(),
            &self.event_q_key.pubkey(),
            &self.coin_fee_receiver,
            &self.pc_fee_receiver,
            10,
        )
        .unwrap();
        let new_block_hash = banks_client
            .to_owned()
            .get_new_blockhash(recent_blockhash)
            .await.unwrap().0;
        wrap_process_transaction(
            vec![match_instruction],
            &payer,
            vec![],
            &new_block_hash,
            &banks_client,
        )
        .await
        .unwrap();
        println!("CHECKEEssFs");

        let consume_instruction = serum_dex::instruction::consume_events(
            serum_program_id,
            open_order_accounts,
            &self.market_key.pubkey(),
            &self.event_q_key.pubkey(),
            &self.coin_fee_receiver,
            &self.pc_fee_receiver,
            10,
        )
        .unwrap();
        wrap_process_transaction(
            vec![consume_instruction],
            &payer,
            vec![],
            &new_block_hash,
            &banks_client,
        )
        .await
        .unwrap();
        println!("CHECKEEssFs 3");

    }
}

pub fn create_token_account(
    payer: &Keypair,
    mint: &Pubkey,
    recent_blockhash: Hash,
    token_account: &Keypair,
    token_account_owner: &Pubkey,
) -> Transaction {
    let instructions = [
        system_instruction::create_account(
            &payer.pubkey(),
            &token_account.pubkey(),
            Rent::default().minimum_balance(165),
            165,
            &spl_token::id(),
        ),
        initialize_account(
            &spl_token::id(),
            &token_account.pubkey(),
            &mint,
            token_account_owner,
        )
        .unwrap(),
    ];
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
    transaction.partial_sign(&[payer, token_account], recent_blockhash);
    transaction
}

#[derive(Debug)]
pub struct OpenOrderView {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub native_coin_free: u64,
    pub native_coin_total: u64,
    pub native_pc_free: u64,
    pub native_pc_total: u64,
    pub orders: Vec<u128>,
}

impl OpenOrderView {
    pub fn parse(data: Vec<u8>) -> Self {
        let stripped = &data[13..];
        let market = Pubkey::new(&stripped[..32]);
        let owner = Pubkey::new(&stripped[32..64]);
        let native_coin_free = u64::from_le_bytes(stripped[64..72].try_into().unwrap());
        let native_coin_total = u64::from_le_bytes(stripped[72..80].try_into().unwrap());
        let native_pc_free = u64::from_le_bytes(stripped[80..88].try_into().unwrap());
        let native_pc_total = u64::from_le_bytes(stripped[88..96].try_into().unwrap());
        let mut orders = Vec::with_capacity(128);
        for i in 0..128 {
            orders.push(u128::from_le_bytes(
                stripped[(128 + 16 * i)..(144 + 16 * i)].try_into().unwrap(),
            ));
        }
        Self {
            market,
            owner,
            native_coin_free,
            native_coin_total,
            native_pc_free,
            native_pc_total,
            orders,
        }
    }

    pub async fn get(key: Pubkey, banks_client: &BanksClient) -> Self {
        Self::parse(
            banks_client
                .to_owned()
                .get_account(key)
                .await
                .unwrap()
                .unwrap()
                .data,
        )
    }
}

pub async fn print_pool_data(
    pool_key: &Pubkey,
    banks_client: &BanksClient,
) -> Result<(), ProgramError> {
    let data = banks_client
        .to_owned()
        .get_account(*pool_key)
        .await
        .unwrap()
        .unwrap()
        .data;
    let pool_assets = unpack_assets(&data[PoolHeader::LEN..])?;
    for asset in pool_assets {
        print!("{:?}", asset);
        let pool_asset_key = get_associated_token_address(&pool_key, &asset.mint_address);
        let asset_data = banks_client
            .to_owned()
            .get_account(pool_asset_key)
            .await
            .unwrap()
            .unwrap()
            .data;
        let token_amount = spl_token::state::Account::unpack(&asset_data)?.amount;
        println!(" Token amount: {:?}", token_amount);
    }

    Ok(())
}

pub fn mint_init_transaction(
    payer: &Keypair,
    mint: &Keypair,
    mint_authority: &Keypair,
    recent_blockhash: Hash,
) -> Transaction {
    let instructions = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            Rent::default().minimum_balance(82),
            82,
            &spl_token::id(),
        ),
        initialize_mint(
            &spl_token::id(),
            &mint.pubkey(),
            &mint_authority.pubkey(),
            None,
            6,
        )
        .unwrap(),
    ];
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
    transaction.partial_sign(&[payer, mint], recent_blockhash);
    transaction
}

pub fn create_and_get_associated_token_address(
    payer_key: &Pubkey,
    parent_key: &Pubkey,
    mint_key: &Pubkey,
) -> (Instruction, Pubkey) {
    let create_source_asset_instruction =
        create_associated_token_account(payer_key, parent_key, mint_key);
    let source_asset_key = get_associated_token_address(parent_key, mint_key);
    return (create_source_asset_instruction, source_asset_key);
}

pub async fn wrap_process_transaction(
    instructions: Vec<Instruction>,
    payer: &Keypair,
    mut signers: Vec<&Keypair>,
    recent_blockhash: &Hash,
    banks_client: &BanksClient,
) -> Result<(), TransportError> {
    let mut setup_transaction = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
    &signers.push(payer);
    setup_transaction.partial_sign(&signers, *recent_blockhash);
    banks_client
        .to_owned()
        .process_transaction(setup_transaction)
        .await
}
