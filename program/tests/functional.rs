#![cfg(feature = "test-bpf")]
use serum_dex::{instruction::SelfTradeBehavior, matching::Side};
use solana_program::{program_pack::Pack, pubkey::Pubkey};

use solana_sdk::{signature::Keypair, signature::Signer};

use spl_token;
use std::{
    convert::TryInto,
    num::{NonZeroU16, NonZeroU64},
    str::FromStr,
};

mod common;

use common::{
    simulation::Actor,
    utils::{
        clone_keypair,
        print_pool_data, Context, OpenOrderView,
    },
};

use common::pool::TestPool;

use common::market::SerumMarket;

const SRM_MINT_KEY: &str = "SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt";

use solana_client::rpc_client::RpcClient;


#[tokio::test]
async fn test_bonfida_bot() {
    let mut ctx = Context::init().await;
    let mints = ctx.get_mints();

    let mut pool = TestPool::new(&ctx);

    for mint_info in &mints {
        pool.add_mint(None, mint_info)
    }

    // Set up Source Owner and Fida mint accounts
    let mut source_actor = Actor {
        key: Keypair::new(),
        asset_accounts: vec![],
        pool_token_balance: 0,
        pool_token_account: None,
        signal_provider: false,
    };

    ctx.refresh_blockhash().await;

    // Initialize the pool
    pool.setup(&ctx).await;

    source_actor.asset_accounts = pool
        .get_funded_token_accounts(&ctx, &source_actor.key.pubkey())
        .await;
    source_actor.pool_token_account =
        Some(pool.get_pt_account(&ctx, &source_actor.key.pubkey()).await);

    let mut signal_provider = Actor {
        key: clone_keypair(&pool.signal_provider),
        asset_accounts: vec![],
        pool_token_balance: 0,
        pool_token_account: None,
        signal_provider: true,
    };
    signal_provider.asset_accounts = pool
        .get_funded_token_accounts(&ctx, &pool.signal_provider.pubkey())
        .await;
    signal_provider.pool_token_account = Some(
        pool.get_pt_account(&ctx, &signal_provider.key.pubkey())
            .await,
    );

    let deposit_amounts = vec![3_238_385, 4_000_000, 1_000_001, 20_000_000];


    // Initialize all asset mints

    // Setup The Serum Dex market
    let pc_mint = pool.mints[2].key;
    let coin_mint = pool.mints[3].key;

    let serum_market = SerumMarket::initialize_market_accounts(&ctx, &coin_mint, &pc_mint)
        .await
        .unwrap();

    // Execute the create pool instruction
    pool.create(
        &ctx,
        source_actor.pool_token_account.as_ref().unwrap(),
        &source_actor.key,
        &source_actor.asset_accounts,
        deposit_amounts,
        &serum_market.market_key.pubkey(),
        604800,
        100
    )
    .await
    .unwrap();

    print_pool_data(&pool.key, &ctx.test_state.banks_client)
        .await
        .unwrap();

    pool.deposit(
        &ctx,
        5000,
        &source_actor.pool_token_account.as_ref().unwrap(),
        &source_actor.key,
        &source_actor.asset_accounts,
    )
    .await
    .unwrap();

    print_pool_data(&pool.key, &ctx.test_state.banks_client)
        .await
        .unwrap();

    let order = pool.initialize_new_order(&ctx).await.unwrap();

    // Execute a CreateOrder instruction
    pool.create_new_order(
        &ctx,
        &serum_market,
        2,
        3,
        &order,
        Side::Bid,
        NonZeroU64::new(1).unwrap(),
        NonZeroU16::new(1 << 14).unwrap(),
    )
    .await
    .unwrap();

    let matching_amount_token = spl_token::state::Account::unpack(
        &ctx.test_state
            .banks_client
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

    print_pool_data(&pool.key, &ctx.test_state.banks_client)
        .await
        .unwrap();

    let mut openorder_view =
        OpenOrderView::get(order.open_orders_account, &ctx.test_state.banks_client).await;

    println!("Open order account before trade: {:?}", openorder_view);
    let matching_open_order = serum_market
        .match_and_crank_order(
            &ctx,
            Side::Bid,
            NonZeroU64::new(2).unwrap(),
            NonZeroU64::new(lots_to_trade).unwrap(),
            0,
            SelfTradeBehavior::DecrementTake,
            &ctx.mint_authority,
            &order.open_orders_account,
        )
        .await;

    // Execute a Settle instruction
    pool.settle(&ctx, &serum_market, 3, 2, &order)
        .await
        .unwrap();

    openorder_view =
        OpenOrderView::get(order.open_orders_account, &ctx.test_state.banks_client).await;
    println!(
        "Open order account after settle before cancel: {:?}",
        openorder_view
    );

    let matching_openorder_view =
        OpenOrderView::get(matching_open_order, &ctx.test_state.banks_client).await;
    println!(
        "Matching Open order account after settle: {:?}",
        matching_openorder_view
    );

    ctx.refresh_blockhash().await;

    // Execute a Cancel order instruction on the original, partially settled, order
    pool.cancel_order(&ctx, &serum_market, &order)
        .await
        .unwrap();

    serum_market
        .crank(&ctx, vec![&order.open_orders_account])
        .await;

    // Settle the cancelled order
    pool.settle(&ctx, &serum_market, 3, 2, &order)
        .await
        .unwrap();

    openorder_view =
        OpenOrderView::get(order.open_orders_account, &ctx.test_state.banks_client).await;
    println!("Open order account after cancel: {:?}", openorder_view);

    print_pool_data(&pool.key, &ctx.test_state.banks_client)
        .await
        .unwrap();

    // Execute a Redeem instruction
    pool.redeem(
        &ctx,
        100,
        &source_actor.key,
        source_actor.pool_token_account.as_ref().unwrap(),
        &source_actor.asset_accounts,
    )
    .await
    .unwrap();

    // pool.collect_fees(&ctx).await.unwrap();

    print_pool_data(&pool.key, &ctx.test_state.banks_client)
        .await
        .unwrap();
}
