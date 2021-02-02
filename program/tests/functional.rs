#![cfg(feature = "test-bpf")]
use std::str::FromStr;
use arrayref::{array_mut_ref, mut_array_refs};
use rand::Rng;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, hash::Hash, instruction::{Instruction, InstructionError}, msg, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_program, sysvar};
use bonfida_bot::{entrypoint::process_instruction, instruction::{create, deposit, init, init_order_tracker}, state::FIDA_MINT_KEY};
use solana_program_test::{BanksClient, ProgramTest, processor};
use solana_sdk::{account::Account, keyed_account, signature::Keypair, signature::Signer, system_instruction, transaction::Transaction};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{self, instruction::{initialize_mint, initialize_account, mint_to}, state::Mint};

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

    // Setup The Serum Dex program
    program_test.add_program(
        "Serum Dex",
        serum_program_id,
        processor!(|a,b,c| {Ok(serum_dex::state::State::process(a, b, c)?)})
    );


    // Start and process transactions on the test network
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;


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
        100
    ).unwrap();
    wrap_process_transaction(
        vec![init_instruction],
        &payer,
        vec![],
        &recent_blockhash,
        &banks_client
    ).await;


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
        let (create_pool_asset_instruction, pool_asset_key) = create_and_get_associated_token_address(
            &payer.pubkey(),
            &pool_key,
            &asset_mint_key
        );
        setup_instructions.push(create_pool_asset_instruction);
        pool_asset_keys.push(pool_asset_key);

        // Source assets
        let (create_source_asset_instruction, source_asset_key) = create_and_get_associated_token_address(
            &payer.pubkey(),
            &source_owner.pubkey(),
            &asset_mint_key
        );
        setup_instructions.push(create_source_asset_instruction);
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
    let (create_target_pooltoken_account, pooltoken_target_key) = create_and_get_associated_token_address(
        &payer.pubkey(),
        &source_owner.pubkey(),
        &mint_key
    );
    setup_instructions.push(create_target_pooltoken_account);
    //Process the setup
    wrap_process_transaction(
        setup_instructions,
        &payer,
        vec![&asset_mint_authority],
        &recent_blockhash,
        &banks_client
    ).await;


    // Execute the create pool instruction
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
&Pubkey::new_unique(),
        deposit_amounts
    ).unwrap();
    wrap_process_transaction(
        vec![create_instruction],
        &payer,
        vec![&source_owner],
        &recent_blockhash,
        &banks_client
    ).await;


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
    ).unwrap();
    wrap_process_transaction(
        vec![deposit_instruction],
        &payer,
        vec![&source_owner],
        &recent_blockhash,
        &banks_client
    ).await;


    // Execute a Init Order Tracker instruction
    // let init_instruction = [init_order_tracker(
    //     &system_program::id(),
    //     &sysvar::rent::id(),
    //     &program_id,
    //     &mint_key,
    //     &payer.pubkey(),
    //     &pool_key,
    //     pool_seeds,
    //     100
    // ).unwrap()
    // ];
    // let mut init_transaction = Transaction::new_with_payer(
    //     &init_instruction,
    //     Some(&payer.pubkey()),
    // );
    // init_transaction.partial_sign(
    //     &[&payer],
    //     recent_blockhash
    // );
    // banks_client.process_transaction(init_transaction).await.unwrap();
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

fn create_and_get_associated_token_address(
    payer_key: &Pubkey,
    parent_key: &Pubkey,
    mint_key: &Pubkey
) -> (Instruction, Pubkey) {
    let create_source_asset_instruction = create_associated_token_account(
        payer_key,
        parent_key,
        mint_key
    );
    let source_asset_key = get_associated_token_address(
        parent_key,
        mint_key
    );
    return (create_source_asset_instruction, source_asset_key)
}

async fn wrap_process_transaction(
    instructions: Vec<Instruction>,
    payer: &Keypair,
    mut signers: Vec<&Keypair>,
    recent_blockhash: &Hash,
    banks_client: &BanksClient,
) {
    let mut setup_transaction = Transaction::new_with_payer(
        &instructions,
        Some(&payer.pubkey()),
    );
    &signers.push(payer);
    setup_transaction.partial_sign(
        &signers,
        *recent_blockhash
    );
    banks_client.to_owned().process_transaction(setup_transaction).await.unwrap();
}