use std::collections::HashMap;

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    decode_error::DecodeError,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::PrintProgramError,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction::create_account,
    sysvar::{clock::Clock, Sysvar},
};
use spl_token::{instruction::transfer, state::Account};
use crate::{instruction::{self, PoolInstruction}, state::{PoolHeader, PoolStatus}};


pub struct Processor {}

impl Processor {
    pub fn process_init(        
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        max_number_of_markets: u32
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let system_program_account = next_account_info(accounts_iter)?;
        let rent_sysvar_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let payer_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        // Find the non reversible public key for the pool account via the seed
        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id).unwrap();
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let state_size = PoolHeader::LEN + max_number_of_markets as usize * instruction::MARKET_DATA_SIZE;

        let init_pool_account = create_account(
            &payer_account.key,
            &pool_key,
            rent.minimum_balance(state_size),
            state_size as u64,
            &program_id,
        );

        invoke_signed(
            &init_pool_account,
            &[
                system_program_account.clone(),
                payer_account.clone(),
                pool_account.clone(),
            ],
            &[&[&pool_seed]],
        )?;
        Ok(())
    }

    pub fn process_create(        
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        signal_provider_key: &Pubkey
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let pool_account = next_account_info(accounts_iter)?;

        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id).unwrap();
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }
        
        // Verifying that no pool was already created with this seed
        let is_initialized =
            pool_account.try_borrow_data()?[PoolHeader::LEN - 1] == 1;

        if is_initialized {
            msg!("Cannot overwrite an existing vesting contract.");
            return Err(ProgramError::InvalidArgument);
        }

        if *pool_account.owner != *program_id {
            msg!("Program should own pool account");
            return Err(ProgramError::InvalidArgument);
        }

        let state_header = PoolHeader {
            signal_provider: *signal_provider_key,
            is_initialized: true,
            status: PoolStatus::UNLOCKED
        };

        let mut data = pool_account.data.borrow_mut();
        state_header.pack_into_slice(&mut data);

        Ok(())
    }

    pub fn process_deposit(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        pool_token_amount: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;

        // pool_header = PoolHeader::unpack(pool_account.data.borrow());
        // let nb_assets = pool_header.size;
        let nb_assets = 2;

        let mut pool_assets_accounts: Vec<&AccountInfo> = vec![];
        let mut source_assets_accounts: Vec<&AccountInfo> = vec![];
        for _ in 0..nb_assets {
            pool_assets_accounts.push(next_account_info(accounts_iter)?)
        }
        let source_owner_account = next_account_info(accounts_iter)?;
        for _ in 0..nb_assets {
            source_assets_accounts.push(next_account_info(accounts_iter)?)
        }

        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id).unwrap();
        // Safety verifications
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }
        if !source_owner_account.is_signer {
            msg!("Source token account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }
        if *pool_account.owner != *program_id {
            msg!("Program should own pool account");
            return Err(ProgramError::InvalidArgument);
        }

        // Compute buy in amount
        let buy_in_amounts: Vec<u64> = vec![];

        // Execute buy in
        for i in 0..nb_assets {
            let instruction = transfer(
                spl_token_account.key,
                source_assets_accounts[i].key,
                pool_assets_accounts[i].key,
                source_owner_account.key,
                &[],
                buy_in_amounts[i],
            )?;
            invoke(
                &instruction,
                &[
                    source_assets_accounts[i].clone(),
                    pool_assets_accounts[i].clone(),
                    spl_token_account.clone(),
                    source_owner_account.clone(),
                ],
            )?;
        }
                
        Ok(())
    }

    pub fn process_instruction(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        msg!("Beginning processing");
        let instruction = PoolInstruction::unpack(instruction_data)?;
        msg!("Instruction unpacked");
        match instruction {
            PoolInstruction::Init {
                pool_seed,
                max_number_of_markets
            } => {
                msg!("Instruction: Init");
                Self::process_init(program_id,
                    accounts,
                    pool_seed,
                    max_number_of_markets)
            },
            PoolInstruction::Create {
                pool_seed,
                signal_provider_key
            } => {
                msg!("Instruction: Create Schedule");
                Self::process_create(
                    program_id,
                    accounts,
                    pool_seed,
                    &signal_provider_key
                )
            },
            PoolInstruction::Deposit {
                pool_seed,
                pool_token_amount
            } => {
                msg!("Instruction: Deposit into Pool");
                Self::process_deposit(
                    program_id,
                    accounts,
                    pool_seed,
                    pool_token_amount
                )
            }
        }
    }
}