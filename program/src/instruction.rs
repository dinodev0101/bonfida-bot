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
        // The seed used to derive the pool account
        pool_seed: [u8; 32],
        max_number_of_markets: u32
    },
    /// Creates a new pool from an empty (uninitialized) one. The two operations need to
    /// be seperated as accound data allocation needs to be first processed
    /// by the network before being overwritten.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The pool account
    Create {
        pool_seed: [u8; 32],
        signal_provider_key: Pubkey
    },
    /// Buy into the pool. The source deposits tokens into the pool and receives
    /// a corresponding amount of pool-token in exchange. The program will try to 
    /// maximize the deposit sum with regards to the amounts given by the source and 
    /// the ratio of tokens present in the pool at that moment. Tokens can only be deposited
    /// in the exact ratio of tokens that are present in the pool.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The spl-token program account
    ///   1. `[]` The pool account
    ///   2..M+2. `[]` The M pool (associated) token assets accounts
    ///   M+3. `[signer]` The source owner account
    ///   M+4..M+K+4. `[]` The K token source token accounts
    Deposit {
        pool_seed: [u8; 32],
        // The amount of pool token the source wishes to buy 
        pool_token_amount: u64
    }
}

impl PoolInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        use BonfidaBotError::InvalidInstruction;
        let (&tag, rest) = input.split_first().ok_or(InvalidInstruction)?;
        Ok(match tag {
            0 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let max_number_of_markets: u32 = rest
                    .get(32..36)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Init {
                    pool_seed,
                    max_number_of_markets
                }
            }
            1 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let signal_provider_key = rest
                    .get(32..64)
                    .and_then(|slice| slice.try_into().ok())
                    .map(Pubkey::new)
                    .ok_or(InvalidInstruction)?;
                Self::Create {
                    pool_seed,
                    signal_provider_key
                }
            }
            2 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .unwrap();
                let pool_token_amount = rest
                    .get(32..40)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Deposit {
                    pool_seed,
                    pool_token_amount
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
                pool_seed,
                max_number_of_markets
            } => {
                buf.push(0);
                buf.extend_from_slice(&pool_seed);
                buf.extend_from_slice(&max_number_of_markets.to_le_bytes());
            }
            Self::Create {
                pool_seed,
                signal_provider_key
            } => {
                buf.push(1);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&signal_provider_key.to_bytes());
            }
            Self::Deposit {
                pool_seed,
                pool_token_amount
            } => {
                buf.push(1);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&pool_token_amount.to_le_bytes());
            }
        };
        buf
    }
}

// Creates a `Init` instruction
pub fn init(
    system_program_id: &Pubkey,
    rent_program_id: &Pubkey,
    bonfidabot_program_id: &Pubkey,
    payer_key: &Pubkey,
    pool_key: &Pubkey,
    pool_seed: [u8; 32],
    max_number_of_markets: u32
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Init {
        pool_seed,
        max_number_of_markets
    }
    .pack();
    let accounts = vec![
        AccountMeta::new_readonly(*system_program_id, false),
        AccountMeta::new_readonly(*rent_program_id, false),
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
    pool_seed: [u8; 32],
    signal_provider_key: &Pubkey
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Create {
        pool_seed,
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

// Creates a `Deposit` instruction
pub fn deposit(
    spl_token_program_id: &Pubkey,
    bonfidabot_program_id: &Pubkey,
    pool_key: &Pubkey,
    pool_assets: &Vec<Pubkey>,
    source_owner: &Pubkey,
    source_token_keys: Vec<Pubkey>,
    pool_seed: [u8; 32],
    pool_token_amount: u64
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Deposit {
        pool_seed,
        pool_token_amount,
    }
    .pack();
    let mut accounts = vec![
        AccountMeta::new(*spl_token_program_id, false),
        AccountMeta::new(*pool_key, false)
    ];
    accounts.append(&mut pool_assets.iter().map(
        |p| AccountMeta::new(*p, false)
    ).collect());
    accounts.push(AccountMeta::new(*source_owner, true));
    accounts.append(&mut source_token_keys.iter().map(
        |p| AccountMeta::new(*p, false)
    ).collect());
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
            pool_seed: [50u8; 32],
            max_number_of_markets: 43,
        };
        assert_eq!(
            original_init,
            PoolInstruction::unpack(&original_init.pack()).unwrap()
        );

        let original_create = PoolInstruction::Create {
            pool_seed: [50u8; 32],
            signal_provider_key: Pubkey::new_unique(),
        };
        let packed_create = original_create.pack();
        let unpacked_create = PoolInstruction::unpack(&packed_create).unwrap();
        assert_eq!(original_create, unpacked_create);

        let original_deposit = PoolInstruction::Deposit {
            pool_seed: [50u8; 32],
            pool_token_amount: 24 as u64,
        };
        let packed_deposit = original_deposit.pack();
        let unpacked_deposit = PoolInstruction::unpack(&packed_deposit).unwrap();
        assert_eq!(original_deposit, unpacked_deposit);
    }
}
