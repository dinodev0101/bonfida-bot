#![cfg(feature = "test-bpf")]
use bonfida_bot::{
    entrypoint::process_instruction,
    instruction::{
        cancel_order, create, create_order, deposit, init, init_order_tracker, settle_funds,
    },
    state::FIDA_MINT_KEY,
};
use rand::Rng;
use serum_dex::{
    instruction::SelfTradeBehavior,
    matching::{OrderType, Side},
    state::{account_parser::TokenAccount, gen_vault_signer_key},
};
use solana_program::{
    entrypoint::ProgramResult, hash::Hash, instruction::Instruction, msg,
    program_error::ProgramError, program_option::COption, program_pack::Pack, pubkey::Pubkey,
    rent::Rent, system_instruction::create_account, system_program, sysvar,
};
use solana_program_test::{find_file, processor, read_file, BanksClient, ProgramTest};
use solana_sdk::{
    account::Account, signature::Keypair, signature::Signer, system_instruction,
    transaction::Transaction, transport::TransportError,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{
    self,
    instruction::{initialize_account, initialize_mint, mint_to},
    state::Mint,
};
use std::{
    convert::TryInto,
    num::{NonZeroU16, NonZeroU64},
    str::FromStr,
};

mod utils;

use utils::{
    create_and_get_associated_token_address, mint_init_transaction, print_pool_data,
    wrap_process_transaction, OpenOrderView, SerumMarket,
};

#[tokio::test]
async fn test_bonfida_bot() {
    println!("asdasdasd");
    // Create program and test environment
    let program_id = Pubkey::from_str("BonfidaBotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    // let serum_program_id = &serum_dex::id();
    let serum_program_id = Pubkey::from_str("SerumDEXotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    let mut pool_seeds = [41u8; 32];
    let mut seed_found = false;
    while !seed_found {
        pool_seeds = rand::thread_rng().gen::<[u8; 32]>();
        let (_, bump) = Pubkey::find_program_address(&[&pool_seeds[..31]], &program_id);
        pool_seeds[31] = bump;
        if Pubkey::create_program_address(&[&pool_seeds, &[1]], &program_id).is_ok() {
            println!("seed found!");
            seed_found = true
        };
    }
    let pool_key = Pubkey::create_program_address(&[&pool_seeds], &program_id).unwrap();
    let mint_key = Pubkey::create_program_address(&[&pool_seeds, &[1]], &program_id).unwrap();
    // Load program
    let mut program_test =
        ProgramTest::new("bonfida_bot", program_id, processor!(process_instruction));

    // Set up Source Owner and Fida mint accounts
    let source_owner = Keypair::new();
    program_test.add_account(
        source_owner.pubkey(),
        Account {
            lamports: 5000000,
            ..Account::default()
        },
    );
    let asset_mint_authority = Keypair::new();
    let mut fida_mint_data = [0; Mint::LEN];
    Mint {
        mint_authority: Some(asset_mint_authority.pubkey()).into(),
        supply: u32::MAX.into(),
        decimals: 6,
        is_initialized: true,
        freeze_authority: None.into(),
    }
    .pack_into_slice(&mut fida_mint_data);
    program_test.add_account(
        Pubkey::from_str(FIDA_MINT_KEY).unwrap(),
        Account {
            lamports: u32::MAX.into(),
            data: fida_mint_data.into(),
            owner: spl_token::id(),
            executable: false,
            ..Account::default()
        },
    );
    let mut serum_mint_data = [0; Mint::LEN];
    Mint {
        mint_authority: Some(asset_mint_authority.pubkey()).into(),
        supply: u32::MAX.into(),
        decimals: 6,
        is_initialized: true,
        freeze_authority: None.into(),
    }
    .pack_into_slice(&mut serum_mint_data);
    let srm_mint = Pubkey::new_unique();
    program_test.add_account(
        srm_mint,
        Account {
            lamports: u32::MAX.into(),
            data: serum_mint_data.into(),
            owner: spl_token::id(),
            executable: false,
            ..Account::default()
        },
    );
    let mut token_data = [0; spl_token::state::Account::LEN];
    spl_token::state::Account {
        mint: Pubkey::from_str("SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt").unwrap(),
        owner: Pubkey::new_unique(),
        amount: u32::MAX.into(),
        delegate: COption::None,
        state: spl_token::state::AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    }
    .pack_into_slice(&mut token_data);
    let srm_receiver = Pubkey::new_unique();
    program_test.add_account(
        srm_receiver,
        Account {
            lamports: u32::MAX.into(),
            data: token_data.into(),
            owner: spl_token::id(),
            executable: false,
            ..Account::default()
        },
    );

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

    // Start and process transactions on the test network
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Setup The Serum Dex market
    let pc_mint = Keypair::new();
    let coin_mint = Keypair::new();
    banks_client
        .process_transaction(mint_init_transaction(
            &payer,
            &pc_mint,
            &asset_mint_authority,
            recent_blockhash,
        ))
        .await
        .unwrap();
    banks_client
        .process_transaction(mint_init_transaction(
            &payer,
            &coin_mint,
            &asset_mint_authority,
            recent_blockhash,
        ))
        .await
        .unwrap();
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
    let init_instruction = init(
        &spl_token::id(),
        &system_program::id(),
        &sysvar::rent::id(),
        &program_id,
        &mint_key,
        &payer.pubkey(),
        &pool_key,
        pool_seeds,
        100,
    )
    .unwrap();
    wrap_process_transaction(
        vec![init_instruction],
        &payer,
        vec![],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    // Setup pool and source token asset accounts
    let deposit_amounts = vec![1_000_001, 20_000_000, 238_479, 2_344, 667];
    let nb_assets = deposit_amounts.len();
    let mut setup_instructions = vec![];
    let mut mint_asset_keys = vec![];
    let mut pool_asset_keys = vec![];
    let mut source_asset_keys = vec![];
    for i in 0..nb_assets {
        // Init asset mint, first asset is FIDA
        let asset_mint_key = match i {
            0 => Pubkey::from_str(FIDA_MINT_KEY).unwrap(),
            1 => pc_mint.pubkey(),
            2 => coin_mint.pubkey(),
            _ => {
                let k = Keypair::new();
                banks_client
                    .process_transaction(mint_init_transaction(
                        &payer,
                        &k,
                        &asset_mint_authority,
                        recent_blockhash,
                    ))
                    .await
                    .unwrap();
                mint_asset_keys.push(k.pubkey());
                k.pubkey()
            }
        };

        //Pool assets
        let (create_pool_asset_instruction, pool_asset_key) =
            create_and_get_associated_token_address(&payer.pubkey(), &pool_key, &asset_mint_key);
        setup_instructions.push(create_pool_asset_instruction);
        pool_asset_keys.push(pool_asset_key);

        // Source assets
        let (create_source_asset_instruction, source_asset_key) =
            create_and_get_associated_token_address(
                &payer.pubkey(),
                &source_owner.pubkey(),
                &asset_mint_key,
            );
        setup_instructions.push(create_source_asset_instruction);
        source_asset_keys.push(source_asset_key);
        setup_instructions.push(
            mint_to(
                &spl_token::id(),
                &asset_mint_key,
                &source_asset_key,
                &asset_mint_authority.pubkey(),
                &[],
                u32::MAX.into(),
            )
            .unwrap(),
        );
    }
    // Init the pooltoken receiving target
    let (create_target_pooltoken_account, pooltoken_target_key) =
        create_and_get_associated_token_address(&payer.pubkey(), &source_owner.pubkey(), &mint_key);
    setup_instructions.push(create_target_pooltoken_account);
    //Process the setup
    wrap_process_transaction(
        setup_instructions,
        &payer,
        vec![&asset_mint_authority],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    // Execute the create pool instruction
    let signal_provider = Keypair::new();
    let create_instruction = create(
        &spl_token::id(),
        &program_id,
        &mint_key,
        &pool_key,
        pool_seeds,
        &pool_asset_keys,
        &pooltoken_target_key,
        &source_owner.pubkey(),
        &source_asset_keys,
        &signal_provider.pubkey(),
        deposit_amounts,
    )
    .unwrap();
    wrap_process_transaction(
        vec![create_instruction],
        &payer,
        vec![&source_owner],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    print_pool_data(&pool_key, &banks_client).await.unwrap();

    // Execute the Deposit transaction
    let deposit_instruction = deposit(
        &spl_token::id(),
        &program_id,
        &mint_key,
        &pool_key,
        &pool_asset_keys,
        &pooltoken_target_key,
        &source_owner.pubkey(),
        &source_asset_keys,
        pool_seeds,
        5000,
    )
    .unwrap();
    wrap_process_transaction(
        vec![deposit_instruction],
        &payer,
        vec![&source_owner],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    print_pool_data(&pool_key, &banks_client).await.unwrap();

    // Execute a Init Order Tracker instruction
    let (open_order, create_open_order_instruction) =
        SerumMarket::create_dex_account(&serum_program_id, &payer.pubkey(), 3216).unwrap();
    let (order_tracker_key, _) =
        Pubkey::find_program_address(&[&pool_seeds, &open_order.pubkey().to_bytes()], &program_id);
    let init_tracker_instruction = init_order_tracker(
        &system_program::id(),
        &sysvar::rent::id(),
        &program_id,
        &order_tracker_key,
        &open_order.pubkey(),
        &payer.pubkey(),
        &pool_key,
        pool_seeds,
    )
    .unwrap();

    wrap_process_transaction(
        vec![create_open_order_instruction, init_tracker_instruction],
        &payer,
        vec![&open_order],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    // Execute a CreateOrder instruction
    let create_order_instruction = create_order(
        &program_id,
        &signal_provider.pubkey(),
        &serum_market.market_key.pubkey(),
        &pool_asset_keys[1],
        1,
        2,
        &open_order.pubkey(),
        &order_tracker_key,
        &serum_market.req_q_key.pubkey(),
        &pool_key,
        &serum_market.coin_vault,
        &serum_market.pc_vault,
        &spl_token::id(),
        &serum_program_id,
        &sysvar::rent::id(),
        None,
        pool_seeds,
        Side::Bid,
        NonZeroU64::new(1).unwrap(),
        NonZeroU16::new(1 << 14).unwrap(),
        serum_dex::matching::OrderType::Limit,
        0,
        SelfTradeBehavior::DecrementTake,
    )
    .unwrap();
    wrap_process_transaction(
        vec![create_order_instruction],
        &payer,
        vec![&signal_provider],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();
    let matching_amount_token = spl_token::state::Account::unpack(
        &banks_client
            .get_account(pool_asset_keys[1])
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

    print_pool_data(&pool_key, &banks_client).await.unwrap();

    let mut openorder_view = OpenOrderView::get(open_order.pubkey(), &banks_client).await;

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
            &asset_mint_authority,
            &open_order.pubkey(),
        )
        .await;
    let after_matching_amount_token = spl_token::state::Account::unpack(
        &banks_client
            .get_account(pool_asset_keys[1])
            .await
            .unwrap()
            .unwrap()
            .data,
    )
    .unwrap()
    .amount;

    // Execute a Settle instruction
    let settle_instruction = settle_funds(
        &program_id,
        &serum_market.market_key.pubkey(),
        &open_order.pubkey(),
        &order_tracker_key,
        &pool_key,
        &mint_key,
        &serum_market.coin_vault,
        &serum_market.pc_vault,
        &pool_asset_keys[2],
        &pool_asset_keys[1],
        &serum_market.vault_signer_pk,
        &spl_token::id(),
        &serum_program_id,
        None,
        pool_seeds,
        1,
        2,
    )
    .unwrap();
    wrap_process_transaction(
        vec![settle_instruction],
        &payer,
        vec![],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();
    openorder_view = OpenOrderView::get(open_order.pubkey(), &banks_client).await;
    println!("Open order account after settle before cancel: {:?}", openorder_view);

    let matching_openorder_view = OpenOrderView::get(matching_open_order, &banks_client).await;
    println!(
        "Matching Open order account after settle: {:?}",
        matching_openorder_view
    );

    // Execute a Cancel order instruction on the original, partially settled, order
    let cancel_instruction = cancel_order(
        &program_id,
        &signal_provider.pubkey(),
        &serum_market.market_key.pubkey(),
        &open_order.pubkey(),
        &serum_market.req_q_key.pubkey(),
        &pool_key,
        &serum_program_id,
        pool_seeds,
        Side::Bid,
        openorder_view.orders[0],
    )
    .unwrap();
    wrap_process_transaction(
        vec![cancel_instruction],
        &payer,
        vec![&signal_provider],
        &recent_blockhash,
        &banks_client,
    )
    .await
    .unwrap();

    serum_market.crank(
        &serum_program_id,
        &recent_blockhash,
        &payer,
        &banks_client,
        vec![&open_order.pubkey()]
    ).await;
    openorder_view = OpenOrderView::get(open_order.pubkey(), &banks_client).await;
    println!("Open order account after cancel: {:?}", openorder_view);

}
