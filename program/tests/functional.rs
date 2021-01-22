#![cfg(feature = "test-bpf")]
use std::str::FromStr;

use solana_program::{hash::Hash,
    pubkey::Pubkey,
    rent::Rent,
    sysvar,
    system_program
};
use bonfida_bot::{entrypoint::process_instruction, instruction::{init, create}};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{account::Account, keyed_account, signature::Keypair, signature::Signer, system_instruction, transaction::Transaction};
use spl_token::{self, instruction::{initialize_mint, initialize_account, mint_to}};

#[tokio::test]
async fn test_bonfida_bot() {
    // Create program and test environment
    let program_id = Pubkey::from_str("BonfidaBotPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();

    let mut seeds = [41u8; 32];
    let (pool_key, bump) = Pubkey::find_program_address(&[&seeds[..31]], &program_id);
    seeds[31] = bump;

    let program_test = ProgramTest::new(
        "bonfida_bot",
        program_id,
        processor!(process_instruction),
    );

    // Start and process transactions on the test network
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    // Initialize the pool
    let init_instruction = [init(
        &system_program::id(),
        &program_id,
        &payer.pubkey(),
        &pool_key,
        seeds,
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

    // Create the pool
    let create_instruction = [create(
        &program_id,
        &pool_key,
        seeds,
&Pubkey::new_unique()
    ).unwrap()
    ];
    let mut create_transaction = Transaction::new_with_payer(
        &create_instruction,
        Some(&payer.pubkey()),
    );
    create_transaction.partial_sign(
        &[&payer],
        recent_blockhash
    );
    banks_client.process_transaction(create_transaction).await.unwrap();

}