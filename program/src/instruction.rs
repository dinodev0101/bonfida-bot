use crate::{error::BonfidaBotError, state::{PoolHeader, unpack_assets}};
use std::{convert::TryInto, mem::size_of, num::{NonZeroU16, NonZeroU64}};
use serum_dex::{instruction::{NewOrderInstructionV2, SelfTradeBehavior}, matching::{OrderType, Side}};
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
        // The maximum number of token asset types the pool will ever be able to hold
        max_number_of_assets: u32
    },
    /// Creates a new pool from an empty (uninitialized) one by performing the first deposit
    /// of any number of different tokens and setting the pubkey of the signal provider.
    /// The first deposit will fix the initial value of 1 pooltoken (credited to the target)
    /// with respect to the deposited tokens.
    /// The init and create operations need to be separated as account data
    /// allocation needs to be first processed by the network before being overwritten.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The spl-token program account
    ///   1. `[]` The pooltoken mint account
    ///   1. `[]` The target account that receives the pooltokens
    ///   0. `[]` The pool account
    ///   2..M+2. `[]` The M pool (associated) token assets accounts in the order of the
    ///      corresponding PoolAssets in the pool account data.
    ///   M+3. `[signer]` The source owner account
    ///   M+4..2M+4. `[]` The M token source token accounts in the same order as above
    Create {
        pool_seed: [u8; 32],
        signal_provider_key: Pubkey,
        deposit_amounts: Vec<u64>
    },
    /// Buy into the pool. The source deposits tokens into the pool and the target receives
    /// a corresponding amount of pool-token in exchange. The program will try to 
    /// maximize the deposit sum with regards to the amounts given by the source and 
    /// the ratio of tokens present in the pool at that moment. Tokens can only be deposited
    /// in the exact ratio of tokens that are present in the pool.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   * Single owner
    ///   0. `[]` The spl-token program account
    ///   1. `[]` The pooltoken mint account
    ///   1. `[]` The target account that receives the pooltokens
    ///   1. `[]` The pool account
    ///   2..M+2. `[]` The M pool (associated) token assets accounts in the order of the
    ///      corresponding PoolAssets in the pool account data.
    ///   M+3. `[signer]` The source owner account
    ///   M+4..2M+4. `[]` The M token source token accounts in the same order as above
    Deposit {
        pool_seed: [u8; 32],
        // The amount of pool token the source wishes to buy
        pool_token_amount: u64
    },
    /// As a signal provider, create a new serum order for the pool.
    /// Amounts are translated into proportions of the pool between 0 and 2**16 - 1
    CreateOrder{
        pool_seed: [u8; 32],
        side: Side,
        limit_price: NonZeroU64,
        max_qty: NonZeroU16,
        order_type: OrderType,
        client_id: u64,
        self_trade_behavior: SelfTradeBehavior
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
                let max_number_of_assets: u32 = rest
                    .get(32..36)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                Self::Init {
                    pool_seed,
                    max_number_of_assets
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
                let mut k = 64;
                let mut deposit_amounts = vec![];
                while k != 0 {
                    match rest.get(k..(k + 8)) {
                        None => k = 0,
                        Some(bytes) => {
                            deposit_amounts.push(u64::from_le_bytes(bytes.try_into().unwrap()));
                            k = k + 8;
                        }
                    }
                }
                Self::Create {
                    pool_seed,
                    signal_provider_key,
                    deposit_amounts
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
            3 => {
                let pool_seed: [u8; 32] = rest
                    .get(..32)
                    .and_then(|slice| slice.try_into().ok())
                    .ok_or(InvalidInstruction)?;
                let side = match rest.get(32).ok_or(InvalidInstruction)?{
                    0 => {Side::Bid}
                    1 => {Side::Ask}
                    _ => {return Err(InvalidInstruction.into())}
                };
                let limit_price = NonZeroU64::new(
                    rest
                        .get(33..41)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u64::from_le_bytes)
                        .ok_or(InvalidInstruction)?)
                    .ok_or(InvalidInstruction)?;
                let max_qty = NonZeroU16::new(
                    rest
                        .get(41..43)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u16::from_le_bytes)
                        .ok_or(InvalidInstruction)?)
                    .ok_or(InvalidInstruction)?;
                
                let order_type = match rest.get(43).ok_or(InvalidInstruction)?{
                    0 => {OrderType::Limit}
                    1 => {OrderType::ImmediateOrCancel}
                    2 => {OrderType::PostOnly}
                    _ => {return Err(InvalidInstruction.into())}
                };
                let client_id = rest
                    .get(44..52)
                    .and_then(|slice| slice.try_into().ok())
                    .map(u64::from_le_bytes)
                    .ok_or(InvalidInstruction)?;
                let self_trade_behavior = match rest.get(52).ok_or(InvalidInstruction)?{
                    0 => {SelfTradeBehavior::DecrementTake}
                    1 => {SelfTradeBehavior::CancelProvide}
                    _ => {return Err(InvalidInstruction.into())}
                };
                Self::CreateOrder {
                    pool_seed,
                    side,
                    limit_price,
                    max_qty,
                    order_type,
                    client_id,
                    self_trade_behavior
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
            Self::Init {
                pool_seed,
                max_number_of_assets
            } => {
                buf.push(0);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&max_number_of_assets.to_le_bytes());
            }
            Self::Create {
                pool_seed,
                signal_provider_key,
                deposit_amounts
            } => {
                buf.push(1);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&signal_provider_key.to_bytes());
                for amount in deposit_amounts.iter() {
                    buf.extend_from_slice(&amount.to_le_bytes());
                }
            }
            Self::Deposit {
                pool_seed,
                pool_token_amount
            } => {
                buf.push(2);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&pool_token_amount.to_le_bytes());
            },
            Self::CreateOrder {
                pool_seed,
                side,
                limit_price,
                max_qty,
                order_type,
                client_id,
                self_trade_behavior   
            } => {
                buf.push(3);
                buf.extend_from_slice(pool_seed);
                buf.extend_from_slice(&match side {
                    Side::Bid => {0u8}
                    Side::Ask => {1}
                }.to_le_bytes());
                buf.extend_from_slice(&limit_price.get().to_le_bytes());
                buf.extend_from_slice(&max_qty.get().to_le_bytes());
                buf.extend_from_slice(&match order_type {
                    OrderType::Limit => {0u8}
                    OrderType::ImmediateOrCancel => {1}
                    OrderType::PostOnly => {2}
                }.to_le_bytes());
                buf.extend_from_slice(&client_id.to_le_bytes());
                buf.extend_from_slice(&match self_trade_behavior {
                    SelfTradeBehavior::DecrementTake => {0u8}
                    SelfTradeBehavior::CancelProvide => {1}
                }.to_le_bytes());
            }
            PoolInstruction::Init { pool_seed, max_number_of_assets } => {}
            PoolInstruction::Create { pool_seed, signal_provider_key, deposit_amounts } => {}
            PoolInstruction::Deposit { pool_seed, pool_token_amount } => {}
            PoolInstruction::CreateOrder { pool_seed, side, limit_price, max_qty, order_type, client_id, self_trade_behavior } => {}
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
    max_number_of_assets: u32
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Init {
        pool_seed,
        max_number_of_assets
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
    signal_provider_key: &Pubkey,
    deposit_amounts: Vec<u64>
) -> Result<Instruction, ProgramError> {
    let data = PoolInstruction::Create {
        pool_seed,
        signal_provider_key: *signal_provider_key,
        deposit_amounts
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
    use std::num::{NonZeroU16, NonZeroU64};

    use serum_dex::{instruction::SelfTradeBehavior, matching::{OrderType, Side}};
    use solana_program::pubkey::Pubkey;

    use super::PoolInstruction;


    #[test]
    fn test_instruction_packing() {
        let original_init = PoolInstruction::Init {
            pool_seed: [50u8; 32],
            max_number_of_assets: 43,
        };
        assert_eq!(
            original_init,
            PoolInstruction::unpack(&original_init.pack()).unwrap()
        );

        let original_create = PoolInstruction::Create {
            pool_seed: [50u8; 32],
            signal_provider_key: Pubkey::new_unique(),
            deposit_amounts: vec![23 as u64, 43 as u64]
        };
        let packed_create = original_create.pack();
        let unpacked_create = PoolInstruction::unpack(&packed_create).unwrap();
        // assert_eq!(original_create, unpacked_create);

        // let original_deposit = PoolInstruction::Deposit {
        //     pool_seed: [50u8; 32],
        //     pool_token_amount: 24 as u64,
        // };
        // let packed_deposit = original_deposit.pack();
        // let unpacked_deposit = PoolInstruction::unpack(&packed_deposit).unwrap();
        // assert_eq!(original_deposit, unpacked_deposit);

        // let original_create_order = PoolInstruction::CreateOrder {
        //     pool_seed: [50u8; 32],
        //     side: Side::Ask,
        //     limit_price: NonZeroU64::new(23).unwrap(),
        //     max_qty: NonZeroU16::new(500).unwrap(),
        //     order_type: OrderType::Limit,
        //     client_id: 0xff44,
        //     self_trade_behavior: SelfTradeBehavior::DecrementTake
        // };
        // let packed_create_order = original_create_order.pack();
        // let unpacked_create_order = PoolInstruction::unpack(&packed_create_order).unwrap();
        // assert_eq!(original_create_order, unpacked_create_order);

    }
}
