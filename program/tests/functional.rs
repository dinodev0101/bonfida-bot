#![cfg(feature = "test-bpf")]
use std::str::FromStr;
use arrayref::{array_mut_ref, mut_array_refs};
use rand::Rng;
use solana_program::{hash::Hash, msg, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_program, sysvar};
use bonfida_bot::{entrypoint::process_instruction, instruction::{init, create, deposit}, state::FIDA_MINT_KEY};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{account::Account, keyed_account, signature::Keypair, signature::Signer, system_instruction, transaction::Transaction};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{self, instruction::{initialize_mint, initialize_account, mint_to}, state::Mint};

#[tokio::test]
async fn test_bonfida_bot() {
    // Create program and test environment
    let program_id = Pubkey::from_str("BonfidaBotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();

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

    let mut program_test = ProgramTest::new(
        "bonfida_bot",
        program_id,
        processor!(process_instruction),
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
    let asset_mint_authority = Keypair::new();
    let mut data = [0; Mint::LEN];
    Mint {
        mint_authority: Some(asset_mint_authority.pubkey()).into(),
        supply: u32::MAX.into(),
        decimals: 6,
        is_initialized: true,
        freeze_authority: None.into(),
    }.pack_into_slice(&mut data);
    program_test.add_account(
        Pubkey::from_str(FIDA_MINT_KEY).unwrap(),
        Account {
            lamports: u32::MAX.into(),
            data: data.into(),
            owner: spl_token::id(),
            executable: false,
            ..Account::default()
        }
    );

    // Start and process transactions on the test network
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;


    // Initialize the pool
    let init_instruction = [init(
        &spl_token::id(),
        &system_program::id(),
        &sysvar::rent::id(),
        &program_id,
        &mint_key,
        &payer.pubkey(),
        &pool_key,
        pool_seeds,
        100
    ).unwrap()
    ];
    let mut init_transaction = Transaction::new_with_payer(
        &init_instruction,
        Some(&payer.pubkey()),
    );
    init_transaction.partial_sign(
        &[&payer],
        recent_blockhash
    );
    banks_client.process_transaction(init_transaction).await.unwrap();


    // Setup pool and source token asset accounts
    let deposit_amounts = vec![1000001, 200, 238479, 2344, 667];
    let nb_assets = deposit_amounts.len();
    let mut setup_instructions = vec![];
    let mut mint_asset_keys = vec![];
    let mut pool_asset_keys = vec![];
    let mut source_asset_keys = vec![];

    for i in 0..nb_assets {
        // Init asset mint, first asset is FIDA
        let asset_mint_key = match i {
            0 => Pubkey::from_str(FIDA_MINT_KEY).unwrap(),
            _ => {
                let k = Keypair::new();
                banks_client.process_transaction(mint_init_transaction(
                    &payer,
                    &k,
                    &asset_mint_authority,
                    recent_blockhash
                )).await.unwrap();
                mint_asset_keys.push(k.pubkey());
                k.pubkey()
            }
        };

        //Pool assets
        let create_pool_asset_instruction = create_associated_token_account(
            &payer.pubkey(),
            &pool_key,
            &asset_mint_key
        );
        setup_instructions.push(create_pool_asset_instruction);
        let pool_asset_key = get_associated_token_address(
            &pool_key,
            &asset_mint_key
        );
        pool_asset_keys.push(pool_asset_key);

        // Source assets
        let create_source_asset_instruction = create_associated_token_account(
            &payer.pubkey(),
            &source_owner.pubkey(),
            &asset_mint_key
        );
        setup_instructions.push(create_source_asset_instruction);
        let source_asset_key = get_associated_token_address(
            &source_owner.pubkey(),
            &asset_mint_key
        );
        source_asset_keys.push(source_asset_key);
        setup_instructions.push(mint_to(
                &spl_token::id(), 
                &asset_mint_key, 
                &source_asset_key, 
                &asset_mint_authority.pubkey(), 
                &[],
                u32::MAX.into()
            ).unwrap()
        );
    }
    // Init the pooltoken receiving target
    setup_instructions.push(create_associated_token_account(
        &payer.pubkey(),
        &source_owner.pubkey(),
        &mint_key
    ));
    let pooltoken_target_key = get_associated_token_address(
        &source_owner.pubkey(),
        &mint_key
    );
    //Process the setup
    let mut setup_transaction = Transaction::new_with_payer(
        &setup_instructions,
        Some(&payer.pubkey()),
    );
    setup_transaction.partial_sign(
        &[&payer, &asset_mint_authority],
        recent_blockhash
    );
    banks_client.process_transaction(setup_transaction).await.unwrap();


    // Execute the create pool instruction
    let create_instruction = [create(
        &spl_token::id(),
        &program_id,
        &mint_key,
        &pool_key,
        pool_seeds,
        &pool_asset_keys,
&pooltoken_target_key,
&source_owner.pubkey(),
        &source_asset_keys,
&Pubkey::new_unique(),
        deposit_amounts
    ).unwrap()
    ];
    let mut create_transaction = Transaction::new_with_payer(
        &create_instruction,
        Some(&payer.pubkey()),
    );
    create_transaction.partial_sign(
        &[&payer, &source_owner],
        recent_blockhash
    );
    banks_client.process_transaction(create_transaction).await.unwrap();


    // Execute the Deposit transaction
    let deposit_instruction = [deposit(
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
    ).unwrap()
    ];
    let mut deposit_transaction = Transaction::new_with_payer(
        &deposit_instruction,
        Some(&payer.pubkey()),
    );
    deposit_transaction.partial_sign(
        &[&payer, &source_owner],
        recent_blockhash
    );
    banks_client.process_transaction(deposit_transaction).await.unwrap();
}

fn mint_init_transaction(
    payer: &Keypair, 
    mint:&Keypair, 
    mint_authority: &Keypair, 
    recent_blockhash: Hash) -> Transaction {
    let instructions = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            Rent::default().minimum_balance(82),
            82,
            &spl_token::id()

        ),
        initialize_mint(
            &spl_token::id(), 
            &mint.pubkey(), 
            &mint_authority.pubkey(),
            None, 
            0
        ).unwrap(),
    ];
    let mut transaction = Transaction::new_with_payer(
        &instructions,
        Some(&payer.pubkey()),
    );
    transaction.partial_sign(
        &[
            payer,
            mint
            ], 
        recent_blockhash
    );
    transaction
}
