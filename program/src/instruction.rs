use crate::{
    state::PoolHeader,
    error::BonfidaBotError
};
use std::{convert::TryInto, mem::size_of};
use solana_program::{account_info::{next_account_info, AccountInfo}, decode_error::DecodeError, entrypoint::ProgramResult, instruction::{AccountMeta, Instruction}, msg, program::{invoke, invoke_signed}, program_error::PrintProgramError, program_error::ProgramError, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_instruction::create_account, sysvar::{Sysvar, clock::Clock, rent}};

pub const MARKET_DATA_SIZE: usize = 10;

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
        seeds: [u8; 32],
        max_number_of_markets: u32
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
                let max_number_of_markets: u32 = rest
                    .get(32..36)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Init {
                    seeds,
                    max_number_of_markets
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
                seeds,
                max_number_of_markets
            } => {
                buf.push(0);
                buf.extend_from_slice(&seeds);
                buf.extend_from_slice(&max_number_of_markets.to_le_bytes());
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
    bonfidabot_program_id: &Pubkey,
    payer_key: &Pubkey,
    pool_key: &Pubkey,
    seeds: [u8; 32],
    max_number_of_markets: u32
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Init {
        seeds,
        max_number_of_markets
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*system_program_id, false),
        AccountMeta::new_readonly(rent::id(), false),
        AccountMeta::new(*pool_key, false),
        AccountMeta::new(*payer_key, true)
    ];
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data
    })
}

// Creates a `CreatePool` instruction
pub fn create(
    bonfidabot_program_id: &Pubkey,
    pool_key: &Pubkey,
    seeds: [u8; 32],
    signal_provider_key: &Pubkey
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Create {
        seeds,
        signal_provider_key: *signal_provider_key
    }
    .pack();
    let accounts = vec![
        AccountMeta::new(*pool_key, false)
    ];
    Ok(Instruction {
        program_id: *bonfidabot_program_id,
        accounts,
        data,
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_instruction_packing() {
        let original_init = PoolInstruction::Init {
            seeds: [50u8; 32],
            max_number_of_markets: 43,
        };
        assert_eq!(
            original_init,
            PoolInstruction::unpack(&original_init.pack()).unwrap()
        );

        let original_create = PoolInstruction::Create {
            seeds: [50u8; 32],
            signal_provider_key: Pubkey::new_unique(),
        };
        let packed_create = original_create.pack();
        let unpacked_create = PoolInstruction::unpack(&packed_create).unwrap();
        assert_eq!(original_create, unpacked_create);

    }
}