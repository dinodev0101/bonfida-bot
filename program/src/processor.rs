use std::{cmp::{max, min}, collections::HashMap, num::{NonZeroU16, NonZeroU64, NonZeroU8}};

use serum_dex::{instruction::{SelfTradeBehavior, initialize_market, new_order}, matching::{OrderType, Side}};
use solana_program::{account_info::{next_account_info, AccountInfo}, decode_error::DecodeError, entrypoint::ProgramResult, msg, program::{invoke, invoke_signed}, program_error::PrintProgramError, program_error::{INVALID_ARGUMENT, ProgramError}, program_pack::{IsInitialized, Pack}, pubkey::Pubkey, rent::Rent, system_instruction::create_account, sysvar::{clock::Clock, Sysvar}};
use spl_token::{instruction::{initialize_mint, mint_to, transfer, initialize_account}, state::Account, state::Mint};
use spl_associated_token_account::get_associated_token_address;
use crate::{error::BonfidaBotError, instruction::{self, PoolInstruction}, state::{FIDA_MINT_KEY, FIDA_MIN_AMOUNT, OrderState, PoolAsset, PoolHeader, PoolStatus, pack_asset, unpack_assets, unpack_unchecked_asset}};

pub struct Processor {}

impl Processor {
    pub fn process_init(        
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        max_number_of_assets: u32
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let system_program_account = next_account_info(accounts_iter)?;
        let rent_sysvar_account = next_account_info(accounts_iter)?;
        let spl_token_program_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let mint_account = next_account_info(accounts_iter)?;
        let payer_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        // Find the non reversible public key for the pool account via the seed
        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id)?;
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        // Find the non reversible public key for the pool mint account via the seed
        let mint_key = Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id)?;
        if mint_key != *mint_account.key {
            msg!("Provided mint account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        let state_size = PoolHeader::LEN + max_number_of_assets as usize * instruction::MARKET_DATA_SIZE;

        let create_pool_account = create_account(
            &payer_account.key,
            &pool_key,
            rent.minimum_balance(state_size),
            state_size as u64,
            &program_id,
        );

        let create_mint_account = create_account(
            &payer_account.key,
            &mint_key,
            rent.minimum_balance(Mint::LEN),
            Mint::LEN as u64,
            &spl_token_program_account.key,
        );

        let init_mint = initialize_mint(
            &spl_token_program_account.key,
            &mint_key,
            &pool_key,
            None,
            6,
        )?;

        invoke_signed(
            &create_pool_account,
            &[
                system_program_account.clone(),
                payer_account.clone(),
                pool_account.clone(),
            ],
            &[&[&pool_seed]],
        )?;

        invoke_signed(
            &create_mint_account,
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

    pub fn process_init_order_tracker(        
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let system_program_account = next_account_info(accounts_iter)?;
        let rent_sysvar_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let openorders_account = next_account_info(accounts_iter)?;
        let payer_account = next_account_info(accounts_iter)?;

        let rent = Rent::from_account_info(rent_sysvar_account)?;

        // Find the non reversible public key for the pool account via the seed
        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id)?;
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }

        // Find the non reversible public key for the pool mint account via the seed
        let order_tracker_key = Pubkey::create_program_address(
            &[&pool_seed, &openorders_account.key.to_bytes()],
            &pool_key
        )?;

        let init_pool_account = create_account(
            &payer_account.key,
            &order_tracker_key,
            rent.minimum_balance(OrderState::LEN),
            OrderState::LEN as u64,
            &pool_key,
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
        signal_provider_key: &Pubkey,
        deposit_amounts: Vec<u64>
    ) -> ProgramResult {
        let number_of_assets = deposit_amounts.len();
        let accounts_iter = &mut accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let mint_account = next_account_info(accounts_iter)?;
        let target_pool_token_account = next_account_info(accounts_iter)?;

        let pool_account = next_account_info(accounts_iter)?;
        let mut pool_assets_accounts: Vec<&AccountInfo> = vec![];
        for _ in 0..number_of_assets {
            pool_assets_accounts.push(next_account_info(accounts_iter)?)
        }
        let source_owner_account = next_account_info(accounts_iter)?;
        let mut source_assets_accounts: Vec<&AccountInfo> = vec![];
        for _ in 0..number_of_assets {
            source_assets_accounts.push(next_account_info(accounts_iter)?)
        }
        
        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id).unwrap();
        let mint_key = Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();
        
        if pool_key != *pool_account.key {
            msg!("Provided pool account is invalid");
            return Err(ProgramError::InvalidArgument);
        }
        if mint_key != *mint_account.key {
            msg!("Provided mint account is invalid");
            return Err(ProgramError::InvalidArgument);
        }
        // Verifying that no pool was already created with this seed
        let pool_status = PoolHeader::unpack_from_slice(
            &pool_account.try_borrow_data()?[..PoolHeader::LEN]
        ).unwrap().status;
        if pool_status != PoolStatus::Uninitialized {
            msg!("Cannot overwrite an existing pool.");
            return Err(ProgramError::InvalidArgument);
        }
        if *pool_account.owner != *program_id {
            msg!("Program should own pool account");
            return Err(ProgramError::InvalidArgument);
        }
        if !source_owner_account.is_signer {
            msg!("Source token account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }
        
        let mut enough_fida = false;
        let mut pool_assets: Vec<PoolAsset> = vec![];
        for i in 0..number_of_assets {
            let mint_key = Account::unpack(&pool_assets_accounts[i as usize].data.borrow())?.mint;
            let pool_asset_key = get_associated_token_address(&pool_key, &mint_key);

            if pool_asset_key != *pool_assets_accounts[i as usize].key {
                msg!("Provided pool asset account is invalid");
                return Err(ProgramError::InvalidArgument);
            }

            // Verify that the first deposit credits more than the min amount of FIDA tokens
            enough_fida =  (pool_assets_accounts[i as usize].key.to_string() == FIDA_MINT_KEY)
                & (deposit_amounts[i] >= FIDA_MIN_AMOUNT);

            let transfer_instruction = transfer(
                spl_token_account.key,
                source_assets_accounts[i as usize].key,
                &pool_assets_accounts[i as usize].key,
                source_owner_account.key,
                &[],
                deposit_amounts[i as usize],
            )?;
            invoke(
                &transfer_instruction,
                &[
                    source_assets_accounts[i as usize].clone(),
                    pool_assets_accounts[i].clone(),
                    spl_token_account.clone(),
                    source_owner_account.clone(),
                ],
            )?;
            pool_assets.push(PoolAsset {
                mint_address: mint_key,
                amount_in_token: deposit_amounts[i as usize],
            });
        }

        if !enough_fida {
            msg!("Pool should always hold at least {:?} FIDA tokens", FIDA_MIN_AMOUNT);
            return Err(ProgramError::InvalidArgument);
        }

        // Mint the first pooltoken to the target
        let instruction = mint_to(
            spl_token_account.key,
            &mint_key,
            target_pool_token_account.key, 
            &pool_key,
            &[],
            1
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

        // Write state header into data
        let state_header = PoolHeader {
            signal_provider: *signal_provider_key,
            status: PoolStatus::Unlocked
        };
        let mut data = pool_account.data.borrow_mut();
        state_header.pack_into_slice(&mut data);
        
        // Write the assets into the account data
        let mut offset = PoolHeader::LEN;
        for asset in pool_assets.iter() {
            asset.pack_into_slice(&mut data[offset..]);
            offset += PoolAsset::LEN;
        }

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
        let mint_account = next_account_info(accounts_iter)?;
        let target_pool_token_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;

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
        let pool_mint_key = Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();
        // Safety verifications
        if pool_key != *pool_account.key {
            msg!("Provided pool account doesn't match the provided pool seed");
            return Err(ProgramError::InvalidArgument);
        }
        if pool_mint_key != *mint_account.key {
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
            let pool_asset_key = get_associated_token_address(
                &pool_key, 
                &pool_assets[i].mint_address);

            if pool_asset_key != *pool_assets_accounts[i as usize].key {
                msg!("Provided pool asset account is invalid");
                return Err(ProgramError::InvalidArgument);
            }

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

        // Mint the effective amount of pooltokens to the target
        let instruction = mint_to(
            spl_token_account.key,
            &pool_mint_key,
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
        self_trade_behavior: SelfTradeBehavior,
        source_index: usize,
        target_index: usize
    ) -> ProgramResult {
        let account_iter = &mut accounts.iter();

        let market = next_account_info(account_iter)?;
        let pool_token_account = next_account_info(account_iter)?;
        let openorders_account = next_account_info(account_iter)?;
        let request_queue = next_account_info(account_iter)?;
        let pool_account = next_account_info(account_iter)?;
        let coin_vault = next_account_info(account_iter)?;
        let pc_vault = next_account_info(account_iter)?;
        let spl_token_program = next_account_info(account_iter)?;
        let rent_sysvar_account = next_account_info(account_iter)?;
        let dex_program_id = next_account_info(account_iter)?;
        let discount_account = next_account_info(account_iter).ok();


        let pool_key = Pubkey::create_program_address(&[&pool_seed], &program_id).unwrap();
        if pool_account.key != &pool_key {
            msg!("Provided pool account doesn't match the provided pool seed")
        }


        let coin_mint = Pubkey::new(&market.data.borrow()[48..80]);
        let pc_mint = Pubkey::new(&market.data.borrow()[80..112]);
        let open_orders_account_owner = Pubkey::new(&openorders_account.data.borrow()[40..72]);

        if &open_orders_account_owner != pool_account.key {
            msg!("The pool account should own the open orders account");
        }

        let source_account = Account::unpack(&pool_token_account.data.borrow())?;
        let source_token_account_key = get_associated_token_address(&pool_key, &source_account.mint);

        if pool_token_account.key != &source_token_account_key {
            msg!("Source token account should be associated to the pool account");
            return Err(ProgramError::InvalidArgument)
        }

        let mut pool_header = PoolHeader::unpack(&pool_account.data.borrow())?;
        match pool_header.status {
            PoolStatus::Uninitialized => { return Err(ProgramError::UninitializedAccount) }
            PoolStatus::Unlocked => { pool_header.status = PoolStatus::PendingOrder(NonZeroU8::new(1).unwrap()) }
            PoolStatus::Locked => { pool_header.status = PoolStatus::LockedPendingOrder(NonZeroU8::new(1).unwrap()) }
            PoolStatus::PendingOrder(n) | PoolStatus::LockedPendingOrder(n) => {
                if n.get() == 64 {
                    msg!("Maximum number of pending orders has been reached. Settle or cancel a pending order.");
                    return Err(BonfidaBotError::Overflow.into())
                }
                let pending_orders = NonZeroU8::new(n.get() + 1).unwrap();
                pool_header.status = match pool_header.status {
                    PoolStatus::PendingOrder(_) => {PoolStatus::PendingOrder(pending_orders)}
                    PoolStatus::LockedPendingOrder(_) => {PoolStatus::LockedPendingOrder(pending_orders)}
                    _ => {unreachable!()}
                }
            }
        }

        let mut source_asset = unpack_unchecked_asset(&pool_account.data.borrow(), source_index)?;
        let mut target_asset = unpack_unchecked_asset(&pool_account.data.borrow(), target_index)?;

        if !source_asset.is_initialized(){
            msg!("The pool has no account at the specificed source index");
            return Err(ProgramError::InvalidArgument)
        }

        if source_asset.mint_address != source_account.mint {
            msg!("Provided coin account does not match the pool source asset")
        }

        if &source_account.owner != pool_account.key {
            msg!("Provided coin account should be owned by the pool")
        }

        let target_mint = match side {
                Side::Bid => {coin_mint}
                Side::Ask => {pc_mint}
            };

        if target_asset.is_initialized(){
            if target_asset.mint_address != target_mint {
                msg!("Target asset does not match bid currency");
                return Err(ProgramError::InvalidArgument)
            }
        } else {
            target_asset.mint_address = target_mint;
            pack_asset(&mut pool_account.data.borrow_mut(), &target_asset, target_index)?;
        }

        if source_asset.mint_address != match side {
            Side::Bid => {pc_mint}
            Side::Ask => {coin_mint}
        } {
            msg!("Wrong source index provided.");
            return Err(ProgramError::InvalidArgument)
        }



        let cast_value: u128 = source_asset.amount_in_token.into();
        let cast_total: u128 = source_account.amount.into();

        let amount_to_trade_in_token = (cast_value.checked_mul(max_qty.get().into()).ok_or(BonfidaBotError::Overflow)? >> 16) as u64;
        let amount_to_trade = (cast_total.checked_mul(max_qty.get().into()).ok_or(BonfidaBotError::Overflow)? >> 16) as u64;

        source_asset.amount_in_token = source_asset.amount_in_token - amount_to_trade_in_token;
        if source_asset.amount_in_token == 0 {
            // Erasing asset
            source_asset.mint_address = Pubkey::new(&[0u8;32]);
        }

        pack_asset(&mut pool_account.data.borrow_mut(), &source_asset, source_index)?;


        let new_order_instruction = new_order(
            market.key,
            openorders_account.key,
            request_queue.key,
            pool_token_account.key,
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
                pool_token_account.clone(),
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
                max_number_of_assets
            } => {
                msg!("Instruction: Init");
                Self::process_init(program_id,
                    accounts,
                    pool_seed,
                    max_number_of_assets)
            },
            PoolInstruction::InitOrderTracker {
                pool_seed,
            } => {
                msg!("Instruction: Init Order Tracker");
                Self::process_init_order_tracker(program_id,
                    accounts,
                    pool_seed,
                )
            },
            PoolInstruction::Create {
                pool_seed,
                signal_provider_key,
                deposit_amounts
            } => {
                msg!("Instruction: Create Pool");
                Self::process_create(
                    program_id,
                    accounts,
                    pool_seed,
                    &signal_provider_key,
                    deposit_amounts
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
                msg!("Instruction: Create Order for Pool");
                Self::process_create_order(
                    program_id, 
                    accounts,
                    pool_seed, 
                    side, 
                    limit_price, 
                    max_qty,
                    order_type, 
                    client_id, 
                    self_trade_behavior,
                    0,
                    0
                )
            }
        }
    }
}