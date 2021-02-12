#![cfg(feature = "test-bpf")]
use bonfida_bot::{
    entrypoint::process_instruction,
    state::FIDA_MINT_KEY,
};
use serum_dex::{instruction::SelfTradeBehavior, matching::Side};
use solana_program::{
    program_pack::Pack, pubkey::Pubkey,
};
use solana_program_test::{ProgramTest, ProgramTestBanksClientExt, find_file, processor, read_file};
use solana_sdk::{account::Account, signature::Keypair, signature::Signer};

use spl_token;
use std::{
    num::{NonZeroU16, NonZeroU64},
    str::FromStr,
};

mod utils;

use utils::{
    add_token_account, create_and_get_associated_token_address, mint_bootstrap,
    print_pool_data, wrap_process_transaction, OpenOrderView,
    SerumMarket, TestPool,
};

const SRM_MINT_KEY: &str = "SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt";

#[test]
fn testitest() {
    let seeds = [0x47 as u8, 0x08, 0x89, 0xc0, 0xda, 0xc2, 0x77, 0xf0, 0x08, 0x40, 0x78, 0x6c, 0xbf, 0x7a, 0x46, 0xe9, 0x46, 0xb9, 0x5c, 0x17, 0x77, 0xd0, 0x1c, 0x63, 0x75, 0x1c, 0x37, 0x51, 0x91, 0x89, 0xda, 0xfe];
    let mint_key = Pubkey::create_program_address(&[&seeds, &[1]], &Pubkey::from_str("4n5939p99bGJRCVPtf2kffKftHRjw6xRXQPcozsVDC77").unwrap());
    println!("{:?}", mint_key.unwrap());
    // 6pKDEbi26VFuhXM6ua3ykxFZwmzKsryFfkRzjxe7drnR
}


#[tokio::test]
async fn test_bonfida_bot() {
    // Create program and test environment
    let program_id = Pubkey::from_str("BonfidaBotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    // let serum_program_id = &serum_dex::id();
    let serum_program_id = Pubkey::from_str("SerumDEXotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    let mut pool = TestPool::new(&program_id);
    // Load program
    let mut program_test =
        ProgramTest::new("bonfida_bot", program_id, processor!(process_instruction));

    // Load The Serum Dex program
    program_test.add_account(
        serum_program_id,
        Account {
            lamports: u32::MAX.into(),
            data: read_file(find_file("serum_dex.so").unwrap()),
            owner: solana_program::bpf_loader_deprecated::id(),
            executable: true,
            ..Account::default()
        },
    );

    // Set up Source Owner and Fida mint accounts
    let source_owner = Keypair::new();

    program_test.add_account(
        source_owner.pubkey(),
        Account {
            lamports: 5000000,
            ..Account::default()
        },
    );
    let deposit_amounts = vec![1_000_001, 20_000_000, 238_479, 2_344, 667];
    let nb_assets = deposit_amounts.len();

    mint_bootstrap(
        Some(SRM_MINT_KEY),
        6,
        &mut program_test,
        &pool.mint_authority.pubkey(),
    );

    // Initialize all asset mints

    for i in 0..nb_assets {
        let (name, address) = match i {
            0 => (Some("FIDA"), Some(FIDA_MINT_KEY)),
            _ => (None, None),
        };
        pool.add_mint(name, address, 6, &mut program_test);
    }

    let srm_receiver = Pubkey::new_unique();
    add_token_account(
        &mut program_test,
        srm_receiver,
        Pubkey::new_unique(),
        Pubkey::from_str(SRM_MINT_KEY).unwrap(),
        u32::MAX.into(),
    );
    // Setup The Serum Dex market
    let pc_mint = pool.mints[1].key;
    let coin_mint = pool.mints[2].key;

    // Start and process transactions on the test network
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    let serum_market = SerumMarket::initialize_market_accounts(
        &serum_program_id,
        &payer,
        &coin_mint,
        &pc_mint,
        recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    // Initialize the pool
    pool.setup(&banks_client, &payer, &recent_blockhash).await;

    // Setup pool and source token asset accounts
    let source_asset_keys = pool
        .get_funded_token_accounts(
            &source_owner.pubkey(),
            &payer,
            &recent_blockhash,
            &banks_client,
        )
        .await;

    let (initialize_target_pt_account_instruction, pooltoken_target_key) =
        create_and_get_associated_token_address(
            &payer.pubkey(),
            &source_owner.pubkey(),
            &pool.mint_key,
        );
    wrap_process_transaction(
        vec![initialize_target_pt_account_instruction],
        &payer,
        vec![&payer],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();
    // Execute the create pool instruction
    pool.create(
        &pooltoken_target_key,
        &source_owner,
        &source_asset_keys,
        deposit_amounts,
        &payer,
        &banks_client,
        &recent_blockhash,
    )
    .await;

    print_pool_data(&pool.key, &banks_client).await.unwrap();

    pool.deposit(
        &pooltoken_target_key,
        &source_owner,
        &source_asset_keys,
        &payer,
        &banks_client,
        &recent_blockhash,
    )
    .await;

    print_pool_data(&pool.key, &banks_client).await.unwrap();

    let order = pool
        .initialize_new_order(&serum_program_id, &payer, &banks_client, &recent_blockhash)
        .await;

    // Execute a CreateOrder instruction
    pool.create_new_order(
        &serum_program_id,
        &payer,
        &banks_client,
        &recent_blockhash,
        &serum_market,
        1,
        2,
        &order,
        NonZeroU64::new(1).unwrap(),
        NonZeroU16::new(1 << 14).unwrap(),
    )
    .await;

    let matching_amount_token = spl_token::state::Account::unpack(
        &banks_client
            .get_account(pool.mints[1].pool_asset_key)
            .await
            .unwrap()
            .unwrap()
            .data,
    )
    .unwrap()
    .amount;
    std::println!("Pool PC asset before trade: {:?}", matching_amount_token);
    let lots_to_trade =
        serum_market.coin_lot_size * matching_amount_token / (serum_market.pc_lot_size * 1); // 1 is price
    println!("Lots to trade for match: {:?}", lots_to_trade);

    print_pool_data(&pool.key, &banks_client).await.unwrap();

    let mut openorder_view = OpenOrderView::get(order.open_orders_account, &banks_client).await;

    println!("Open order account before trade: {:?}", openorder_view);
    let matching_open_order = serum_market
        .match_and_crank_order(
            &serum_program_id,
            &payer,
            recent_blockhash,
            &banks_client,
            Side::Bid,
            NonZeroU64::new(2).unwrap(),
            NonZeroU64::new(lots_to_trade).unwrap(),
            0,
            SelfTradeBehavior::DecrementTake,
            &pool.mint_authority,
            &order.open_orders_account,
        )
        .await;

    // Execute a Settle instruction
    pool.settle(
        &serum_program_id,
        &payer,
        &banks_client,
        &recent_blockhash,
        &serum_market,
        2,
        1,
        &order,
    )
    .await;

    openorder_view = OpenOrderView::get(order.open_orders_account, &banks_client).await;
    println!(
        "Open order account after settle before cancel: {:#?}",
        openorder_view
    );

    let matching_openorder_view = OpenOrderView::get(matching_open_order, &banks_client).await;
    println!(
        "Matching Open order account after settle: {:#?}",
        matching_openorder_view
    );

    // Execute a Cancel order instruction on the original, partially settled, order
    pool.cancel_order(
        &serum_program_id,
        &payer,
        &banks_client,
        &recent_blockhash,
        &serum_market,
        &order,
    )
    .await;

    serum_market
        .crank(
            &serum_program_id,
            &recent_blockhash,
            &payer,
            &banks_client,
            vec![&order.open_orders_account],
        )
        .await;

    // Settle the cancelled order
    pool.settle(
        &serum_program_id,
        &payer,
        &banks_client.to_owned(),
        &banks_client.get_new_blockhash(&recent_blockhash).await.unwrap().0,
        &serum_market,
        2,
        1,
        &order,
    )
    .await;

    openorder_view = OpenOrderView::get(order.open_orders_account, &banks_client).await;
    println!("Open order account after cancel: {:#?}", openorder_view);

    print_pool_data(&pool.key, &banks_client).await.unwrap();

    // Execute a Redeem instruction
    pool.redeem(&payer, &source_owner, &banks_client, &recent_blockhash, &pooltoken_target_key, &source_asset_keys).await;

    print_pool_data(&pool.key, &banks_client).await.unwrap();
}
