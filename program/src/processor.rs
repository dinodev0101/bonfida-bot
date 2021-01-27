use std::{cmp::{max, min}, collections::HashMap, num::{NonZeroU16, NonZeroU64}};

use serum_dex::{instruction::{SelfTradeBehavior, initialize_market, new_order}, matching::{OrderType, Side}};
use solana_program::{account_info::{next_account_info, AccountInfo}, decode_error::DecodeError, entrypoint::ProgramResult, msg, program::{invoke, invoke_signed}, program_error::PrintProgramError, program_error::{INVALID_ARGUMENT, ProgramError}, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_instruction::create_account, sysvar::{clock::Clock, Sysvar}};
use spl_token::{instruction::{initialize_mint, mint_to, transfer}, state::Account, state::Mint};
use crate::{error::BonfidaBotError, instruction::{self, PoolInstruction}, state::{PoolAsset, PoolHeader, PoolStatus, unpack_assets}};


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
        let spl_token_program_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let mint_account = next_account_info(accounts_iter)?;
        let payer_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        if spl_token_program_account.key != &spl_token::id(){
            msg!("Invalid spl token program account");
            return Err(ProgramError::InvalidArgument);
        }

        // Find the non reversible public key for the pool account via the seed
        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id)?;
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        // Find the non reversible public key for the pool account via the seed
        let mint_key = Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id)?;
        if mint_key != *mint_account.key {
            msg!("Provided mint account is invalid");
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

        let init_mint_account = create_account(
            &payer_account.key,
            &pool_key,
            rent.minimum_balance(Mint::LEN),
            Mint::LEN as u64,
            &spl_token_program_account.key,
        );

        let init_mint = initialize_mint(
            &spl_token_program_account.key,
            &pool_key,
            &pool_key,
            None,
            6,
        )?;

        invoke_signed(
            &init_pool_account,
            &[
                system_program_account.clone(),
                payer_account.clone(),
                pool_account.clone(),
            ],
            &[&[&pool_seed]],
        )?;

        invoke_signed(
            &init_mint_account,
            &[
                system_program_account.clone(),
                payer_account.clone(),
                mint_account.clone()
            ],
            &[&[&pool_seed, &[1]]]
        )?;

        invoke(
            &init_mint,
            &[
                mint_account.clone(),
                rent_sysvar_account.clone()
            ]
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
            status: PoolStatus::Unlocked
        };

        let mut data = pool_account.data.borrow_mut();
        state_header.pack_into_slice(&mut data);

        Ok(())
    }

    pub fn process_deposit(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        // The amount of pooltokens wished to be bought
        pool_token_amount: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let mint_account = next_account_info(accounts_iter)?;
        let target_pool_token_account = next_account_info(accounts_iter)?;

        let pool_header = PoolHeader::unpack(&pool_account.data.borrow()[..PoolHeader::LEN])?;
        let pool_assets = unpack_assets(&pool_account.data.borrow()[PoolHeader::LEN..])?;
        let nb_assets = pool_assets.len();

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
        let mint_key = Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();
        // Safety verifications
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        if mint_key != *mint_account.key {
            msg!("Provided mint account is invalid");
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
        if pool_header.status == PoolStatus::Locked {
            msg!("Pool is currently locked by signal provider, \
                no buy-ins are possible");
            return Err(ProgramError::InvalidArgument);
        }



        // Compute buy-in amount. The effective buy-in amount can be less than the
        // input_token_amount as the source accounts need to satisfy the pool asset ratios
        let mut pool_token_effective_amount = std::u64::MAX;
        for i in 0..nb_assets {
            let source_asset_amount = Account::unpack(&source_assets_accounts[i].data.borrow())?.amount;
            pool_token_effective_amount = min(
                source_asset_amount.checked_div(pool_assets[i].amount_in_token).unwrap_or(std::u64::MAX), 
                pool_token_effective_amount
            );
        }
        pool_token_effective_amount = min(pool_token_amount, pool_token_effective_amount);
        
        // Execute buy in
        for i in 0..nb_assets {
            let amount = pool_token_effective_amount
                .checked_mul(pool_assets[i].amount_in_token)
                .ok_or(BonfidaBotError::Overflow)?;
            if amount == 0 {
                break;
            }
            let instruction = transfer(
                spl_token_account.key,
                source_assets_accounts[i].key,
                pool_assets_accounts[i].key,
                source_owner_account.key,
                &[],
                amount,
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

        let instruction = mint_to(
            spl_token_account.key, 
            &mint_key, 
            target_pool_token_account.key, 
            &pool_key, 
            &[], 
            pool_token_effective_amount
        )?;

        invoke_signed(
            &instruction, 
            &[
                spl_token_account.clone(),
                mint_account.clone(),
                target_pool_token_account.clone(),
                pool_account.clone()
            ], 
            &[&[&pool_seed]],
        )?;

        Ok(())
    }

    pub fn process_create_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        side:Side,
        limit_price: NonZeroU64,
        max_qty: NonZeroU16,
        order_type: OrderType,
        client_id: u64,
        self_trade_behavior: SelfTradeBehavior
    ) -> ProgramResult {
        let account_iter = &mut accounts.iter();

        let market = next_account_info(account_iter)?;
        let pool_token_account = next_account_info(account_iter)?;
        let openorders_account = next_account_info(account_iter)?;
        let request_queue = next_account_info(account_iter)?;
        let payer = next_account_info(account_iter)?;
        let pool_account = next_account_info(account_iter)?;
        let coin_vault = next_account_info(account_iter)?;
        let pc_vault = next_account_info(account_iter)?;
        let spl_token_program = next_account_info(account_iter)?;
        let rent_sysvar_account = next_account_info(account_iter)?;
        let dex_program_id = next_account_info(account_iter)?;
        let discount_account = next_account_info(account_iter).ok();

        let coin_mint = Pubkey::new(&market.data.borrow()[48..80]);
        let pc_mint = Pubkey::new(&market.data.borrow()[80..112]);

        let pool_header = PoolHeader::unpack(&pool_account.data.borrow())?;
        let pool_assets = unpack_assets(&pool_account.data.borrow()[PoolHeader::LEN..])?;

        let asset_index = pool_assets
            .binary_search_by_key(&coin_mint, |a| {a.mint_address})
            .map_err(|_| {ProgramError::InvalidArgument})?;

        let mut coin_asset = pool_assets[asset_index].clone();
        let coin_account = Account::unpack(&pool_token_account.data.borrow())?;

        let cast_value: u128 = coin_asset.amount_in_token.into();
        let cast_total: u128 = coin_account.amount.into();

        let amount_to_trade = (cast_total.checked_mul(max_qty.get().into()).ok_or(BonfidaBotError::Overflow)? >> 16) as u64;
        let new_amount_in_token = (cast_value.checked_mul(max_qty.get().into()).ok_or(BonfidaBotError::Overflow)? >> 16) as u64;

        coin_asset.amount_in_token = new_amount_in_token;

        coin_asset.pack_into_slice(&mut pool_account.data.borrow_mut()[PoolHeader::LEN + asset_index * PoolAsset::LEN..]);


        let new_order_instruction = new_order(
            market.key,
            openorders_account.key,
            request_queue.key,
            payer.key,
            pool_account.key,
            coin_vault.key,
            pc_vault.key,
            spl_token_program.key,
            rent_sysvar_account.key,
            discount_account.map(|account| {account.key}),
            dex_program_id.key,
            side,
            limit_price,
            NonZeroU64::new(amount_to_trade).unwrap(),
            order_type,
            client_id,
            self_trade_behavior
        )?;

        let mut account_infos = vec![
                market.clone(),
                openorders_account.clone(),
                request_queue.clone(),
                payer.clone(),
                pool_account.clone(),
                coin_vault.clone(),
                pc_vault.clone(),
                spl_token_program.clone(),
                rent_sysvar_account.clone()
        ];

        if let Some(account) = discount_account {
            account_infos.push(account.clone());
        }

        invoke_signed(
            &new_order_instruction,
            &account_infos,
            &[&[&pool_seed]]
        )?;

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
            PoolInstruction::CreateOrder {
                pool_seed,
                side,
                limit_price,
                max_qty,
                order_type,
                client_id,
                self_trade_behavior
            } => {
                msg!("Instruction: Deposit into Pool");
                Self::process_create_order(
                    program_id, 
                    accounts,
                    pool_seed, 
                    side, 
                    limit_price, 
                    max_qty,
                    order_type, 
                    client_id, 
                    self_trade_behavior
                )
            }
        }
    }
}