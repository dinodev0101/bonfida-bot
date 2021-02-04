#![cfg(feature = "test-bpf")]
use bonfida_bot::{
    entrypoint::process_instruction,
    instruction::{create, deposit, init, init_order_tracker, create_order, cancel_order, settle_funds, redeem},
    state::FIDA_MINT_KEY,
};
use rand::{Rng, rngs::OsRng};
use serum_dex::{instruction::SelfTradeBehavior, matching::Side, state::{account_parser::TokenAccount, gen_vault_signer_key}};
use solana_program::{hash::Hash, instruction::Instruction, program_error::ProgramError, program_option::COption, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_instruction::create_account, system_program, sysvar};
use solana_program_test::{BanksClient, ProgramTest, find_file, processor, read_file};
use solana_sdk::{
    account::Account, signature::Keypair, signature::Signer, system_instruction,
    transaction::Transaction,
};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{
    self,
    instruction::{initialize_mint, mint_to, initialize_account},
    state::Mint,
};
use std::{num::{NonZeroU16, NonZeroU64}, str::FromStr};

#[tokio::test]
async fn test_bonfida_bot() {
    // Create program and test environment
    let program_id = Pubkey::from_str("BonfidaBotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    let serum_program_id = Pubkey::from_str("SerumDEXotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    // TODO init order tracker
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
        }
    );
    
    // Start and process transactions on the test network
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;


    // Setup The Serum Dex market
    let pc_mint = Keypair::new();
    let coin_mint = Keypair::new();
    banks_client.process_transaction(mint_init_transaction(
        &payer,
        &pc_mint,
        &asset_mint_authority,
        recent_blockhash,
    ))
    .await
    .unwrap();
    banks_client.process_transaction(mint_init_transaction(
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
        &banks_client
    ).await.unwrap();


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
    .await;

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
    .await;

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
    .await;

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
        2,
    )
    .unwrap();
    wrap_process_transaction(
        vec![deposit_instruction],
        &payer,
        vec![&source_owner],
        &recent_blockhash,
        &banks_client,
    )
    .await;

    // Execute a Init Order Tracker instruction
    let (open_order, create_order_tracker_instruction) = SerumMarket::create_dex_account(
        &serum_program_id, 
        &payer.pubkey(), 
        2184).unwrap();
        let (order_tracker_key, _) = Pubkey::find_program_address(
            &[&pool_seeds, &open_order.pubkey().to_bytes()],
        &program_id,
    );
    let init_tracker_instruction = init_order_tracker(
        &system_program::id(),
        &sysvar::rent::id(),
        &program_id,
        &order_tracker_key,
        &open_order.pubkey(),
        &payer.pubkey(),
        &pool_key,
        pool_seeds,
    ).unwrap();

    wrap_process_transaction(
        vec![create_order_tracker_instruction, init_tracker_instruction],
        &payer,
        vec![&open_order],
        &recent_blockhash,
        &banks_client,
    )
    .await;

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
        NonZeroU16::new(10).unwrap(), 
        serum_dex::matching::OrderType::Limit,
        0, 
        SelfTradeBehavior::DecrementTake,
    ).unwrap();
    wrap_process_transaction(
        vec![create_order_instruction],
        &payer,
        vec![&signal_provider],
        &recent_blockhash,
        &banks_client,
    )
    .await;

    // // Execute a CreateOrder instruction that matches the previous order
    // let create_matching_order_instruction = create_order(
    //     &program_id, 
    //     &signal_provider.pubkey(), 
    //     &serum_market.market_key.pubkey(), 
    //     &pool_asset_keys[1], 
    //     0,
    //     0,
    //     &open_order.pubkey(), 
    //     &order_tracker_key, 
    //     &serum_market.req_q_key.pubkey(),
    //     &pool_key, 
    //     &serum_market.coin_vault, 
    //     &serum_market.pc_vault, 
    //     &spl_token::id(), 
    //     &serum_program_id, 
    //     &sysvar::rent::id(),
    //     None, 
    //     pool_seeds, 
    //     Side::Bid, 
    //     NonZeroU64::new(1).unwrap(), 
    //     NonZeroU16::new(10).unwrap(), 
    //     serum_dex::matching::OrderType::Limit,
    //     0, 
    //     SelfTradeBehavior::DecrementTake,
    // ).unwrap();
    // wrap_process_transaction(
    //     vec![create_matching_order_instruction],
    //     &payer,
    //     vec![&signal_provider],
    //     &recent_blockhash,
    //     &banks_client,
    // )
    // .await;

    // Crank the Serum matching
    let consume_instruction = serum_dex::instruction::consume_events(
        &serum_program_id,
        vec![&open_order.pubkey()],
        &serum_market.market_key.pubkey(),
        &serum_market.event_q_key.pubkey(),
        &srm_receiver,
        &srm_receiver,
        1
    ).unwrap();
    wrap_process_transaction(
        vec![consume_instruction],
        &payer,
        vec![],
        &recent_blockhash,
        &banks_client,
    )
    .await;
}

fn mint_init_transaction(
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

fn create_and_get_associated_token_address(
    payer_key: &Pubkey,
    parent_key: &Pubkey,
    mint_key: &Pubkey,
) -> (Instruction, Pubkey) {
    let create_source_asset_instruction =
        create_associated_token_account(payer_key, parent_key, mint_key);
    let source_asset_key = get_associated_token_address(parent_key, mint_key);
    return (create_source_asset_instruction, source_asset_key);
}

async fn wrap_process_transaction(
    instructions: Vec<Instruction>,
    payer: &Keypair,
    mut signers: Vec<&Keypair>,
    recent_blockhash: &Hash,
    banks_client: &BanksClient,
) {
    let mut setup_transaction = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
    &signers.push(payer);
    setup_transaction.partial_sign(&signers, *recent_blockhash);
    banks_client
        .to_owned()
        .process_transaction(setup_transaction)
        .await
        .unwrap();
}

struct SerumMarket {
    market_key: Keypair,
    req_q_key: Keypair,
    event_q_key: Keypair,
    bids_key: Keypair,
    asks_key: Keypair,
    vault_signer_pk: Pubkey,
    vault_signer_nonce: u64,
    coin_vault: Pubkey,
    pc_vault: Pubkey
}

impl SerumMarket {

    async fn initialize_market_accounts(
        serum_program_id: &Pubkey,
        payer: &Keypair,
        coin_mint: &Keypair,
        pc_mint: &Keypair,
        recent_blockhash: Hash,
        banks_client: &BanksClient
    ) -> Result<Self, ProgramError> {
        println!("--- Creating Market accounts ---");
        let (market_key, create_market) = Self::create_dex_account(serum_program_id, &payer.pubkey(), 376)?;
        let (req_q_key, create_req_q) = Self::create_dex_account(serum_program_id, &payer.pubkey(), 640)?;
        let (event_q_key, create_event_q) = Self::create_dex_account(serum_program_id, &payer.pubkey(), 1 << 20)?;
        let (bids_key, create_bids) = Self::create_dex_account(serum_program_id, &payer.pubkey(), 1 << 16)?;
        let (asks_key, create_asks) = Self::create_dex_account(serum_program_id, &payer.pubkey(), 1 << 16)?;
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
        let keys = vec![
            &market_key,
            &req_q_key,
            &event_q_key,
            &bids_key,
            &asks_key,
        ];
        wrap_process_transaction(
            create_instructions,
            &payer,
            keys,
            &recent_blockhash,
            &banks_client,
        )
        .await;

        let coin_vault = Keypair::new();
        let pc_vault = Keypair::new();
        let create_coin_vault = create_token_account(
            &payer, 
            coin_mint,
            recent_blockhash,
            &coin_vault,
            &vault_signer_pk
        );
        banks_client.to_owned().process_transaction(create_coin_vault).await.unwrap();
        let create_pc_vault = create_token_account(
            &payer, 
            pc_mint, 
            recent_blockhash,
            &pc_vault,
            &vault_signer_pk
        );
        banks_client.to_owned().process_transaction(create_pc_vault).await.unwrap();
        
        println!("--- Initializing Market ---");
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
        let info = SerumMarket {
            market_key,
            req_q_key,
            event_q_key,
            bids_key,
            asks_key,
            vault_signer_pk,
            vault_signer_nonce,
            coin_vault: coin_vault.pubkey(),
            pc_vault: pc_vault.pubkey()
        };
        wrap_process_transaction(
            vec![init_market_instruction],
            &payer,
            vec![],
            &recent_blockhash,
            &banks_client,
        )
        .await;

        Ok(info)
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
} 

fn create_token_account(
    payer: &Keypair, 
    mint:&Keypair, 
    recent_blockhash: Hash,
    token_account:&Keypair,
    token_account_owner: &Pubkey
) -> Transaction {
    let instructions = [
        system_instruction::create_account(
            &payer.pubkey(),
            &token_account.pubkey(),
            Rent::default().minimum_balance(165),
            165,
            &spl_token::id()
        ),
        initialize_account(
            &spl_token::id(), 
            &token_account.pubkey(), 
            &mint.pubkey(), 
            token_account_owner
        ).unwrap()
   ];
   let mut transaction = Transaction::new_with_payer(
    &instructions,
    Some(&payer.pubkey()),
    );
    transaction.partial_sign(
        &[
            payer,
            token_account
            ], 
        recent_blockhash
    );
    transaction
}