use crate::{
    state::PoolHeader,
    error::BonfidaBotError
};
use std::{convert::TryInto, mem::size_of};
use solana_program::{account_info::{next_account_info, AccountInfo}, decode_error::DecodeError, entrypoint::ProgramResult, instruction::{AccountMeta, Instruction}, msg, program::{invoke, invoke_signed}, program_error::PrintProgramError, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_instruction::create_account, sysvar::{Sysvar, clock::Clock, rent}};

pub const POOL_HEADER_SIZE: usize = 33;
pub const POOL_NB_MARKETS: usize = 100; //TODO 
pub const MARKET_DATA_SIZE: usize = 10;
pub const POOL_DATA_SIZE: usize = PoolHeader::LEN + POOL_NB_MARKETS * MARKET_DATA_SIZE;

#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub enum PoolInstruction {
    /// Initializes an empty pool account for the bonfida-bot program
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The system program account
    ///   1. `[signer]` The fee payer account
    Init {
        // The seed used to derive the vesting accounts address
        seeds: [u8; 32]
    },
    /// Creates a new pool from an empty one. The two operations need to
    /// be seperated as accound data allocation needs to be first processed
    /// by the network before being overwritten.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The pool account
    Create {
        seeds: [u8; 32],
        signal_provider_key: Pubkey
    }
}

impl PoolInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        use BonfidaBotError::InvalidInstruction;
        let (&tag, rest) = input.split_first().ok_or(InvalidInstruction)?;
        Ok(match tag {
            0 => {
                let seeds: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                Self::Init {
                    seeds
                }
            }
            1 => {
                let seeds: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let signal_provider_key = rest
                    .get(32..64)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                Self::Create {
                    seeds,
                    signal_provider_key
                }
            }
            _ => {
                msg!("Unsupported tag");
                return Err(InvalidInstruction.into());
            }
        })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match self {
            &Self::Init {
                seeds
            } => {
                buf.push(0);
                buf.extend_from_slice(&seeds);
            }
            Self::Create {
                seeds,
                signal_provider_key
            } => {
                buf.push(1);
                buf.extend_from_slice(seeds);
                buf.extend_from_slice(&signal_provider_key.to_bytes());
            }
        };
        buf
    }
}

// Creates a `Init` instruction
pub fn init(
    system_program_id: &Pubkey,
    vesting_program_id: &Pubkey,
    payer_key: &Pubkey,
    vesting_account: &Pubkey,
    seeds: [u8; 32]
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Init {
        seeds
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*system_program_id, false),
        AccountMeta::new_readonly(rent::id(), false),
        AccountMeta::new(*payer_key, true),
        AccountMeta::new(*vesting_account, false)
    ];
    Ok(Instruction {
        program_id: *vesting_program_id,
        accounts,
        data
    })
}

// Creates a `CreateSchedule` instruction
pub fn create(
    vesting_program_id: &Pubkey,
    token_program_id: &Pubkey,
    vesting_account_key: &Pubkey,
    vesting_token_account_key: &Pubkey,
    source_token_account_owner_key: &Pubkey,
    source_token_account_key: &Pubkey,
    destination_token_account_key: &Pubkey,
    mint_address: &Pubkey,
    schedules: Vec<Schedule>,
    seeds: [u8; 32],
) -> Result<Instruction, ProgramError> {
    let data = VestingInstruction::Create {
        mint_address: *mint_address,
        seeds,
        destination_token_address: *destination_token_account_key,
        schedules,
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*token_program_id, false),
        AccountMeta::new(*vesting_account_key, false),
        AccountMeta::new(*vesting_token_account_key, false),
        AccountMeta::new_readonly(*source_token_account_owner_key, true),
        AccountMeta::new(*source_token_account_key, false),
    ];
    Ok(Instruction {
        program_id: *vesting_program_id,
        accounts,
        data,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_instruction_packing() {
        let mint_address = Pubkey::new_unique();
        let destination_token_address = Pubkey::new_unique();

        let original_create = VestingInstruction::Create {
            seeds: [50u8; 32],
            schedules: vec![Schedule {
                amount: 42,
                release_height: 250,
            }],
            mint_address: mint_address.clone(),
            destination_token_address,
        };
        let packed_create = original_create.pack();
        let unpacked_create = VestingInstruction::unpack(&packed_create).unwrap();
        assert_eq!(original_create, unpacked_create);

        let original_unlock = VestingInstruction::Unlock { seeds: [50u8; 32] };
        assert_eq!(
            original_unlock,
            VestingInstruction::unpack(&original_unlock.pack()).unwrap()
        );

        let original_init = VestingInstruction::Init {
            number_of_schedules: 42,
            seeds: [50u8; 32],
        };
        assert_eq!(
            original_init,
            VestingInstruction::unpack(&original_init.pack()).unwrap()
        );

        let original_change = VestingInstruction::ChangeDestination { seeds: [50u8; 32] };
        assert_eq!(
            original_change,
            VestingInstruction::unpack(&original_change.pack()).unwrap()
        );
    }
}