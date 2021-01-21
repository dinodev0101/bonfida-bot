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
use crate::{
    instruction::{
        PoolInstruction,
        POOL_DATA_SIZE
    },
    state::PoolHeader
};


pub struct Processor {}

impl Processor {
    pub fn process_init(        
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        seeds: [u8; 32],
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let system_program_account = next_account_info(accounts_iter)?;
        let rent_sysvar_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let payer_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        // Find the non reversible public key for the pool account via the seed
        let pool_key = Pubkey::create_program_address(&[&seeds], &program_id).unwrap();
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let init_pool_account = create_account(
            &payer_account.key,
            &pool_key,
            rent.minimum_balance(POOL_DATA_SIZE),
            POOL_DATA_SIZE as u64,
            &program_id,
        );

        invoke_signed(
            &init_pool_account,
            &[
                system_program_account.clone(),
                payer_account.clone(),
                pool_account.clone(),
            ],
            &[&[&seeds]],
        )?;
        Ok(())
    }

    pub fn process_create(        
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        seeds: [u8; 32],
        signal_provider_key: &Pubkey
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let pool_account = next_account_info(accounts_iter)?;

        let pool_key = Pubkey::create_program_address(&[&seeds], &program_id).unwrap();
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if *pool_account.owner != *program_id {
            msg!("Program should own pool account");
            return Err(ProgramError::InvalidArgument);
        }

        let state_header = PoolHeader {
            signal_provider: *signal_provider_key,
            is_initialized: true
        };

        let mut data = pool_account.data.borrow_mut();
        state_header.pack_into_slice(&mut data);

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
                seeds,
            } => {
                msg!("Instruction: Init");
                Self::process_init(program_id, accounts, seeds)
            },
            PoolInstruction::Create {
                seeds,
                signal_provider_key
            } => {
                msg!("Instruction: Create Schedule");
                Self::process_create(
                    program_id,
                    accounts,
                    seeds,
                    &signal_provider_key
                )
            }
        }
    }
}