use std::{cmp::min, convert::TryInto, num::{NonZeroU16, NonZeroU64, NonZeroU8}};

use crate::{
    error::BonfidaBotError,
    instruction::{self, PoolInstruction},
    state::{
        pack_asset, unpack_assets, unpack_unchecked_asset, OrderTracker, PoolAsset, PoolHeader,
        PoolStatus, FIDA_MINT_KEY, FIDA_MIN_AMOUNT,
    },
    utils::{
        check_open_orders_account, check_order_tracker, check_pool_key, check_signal_provider,
    },
};
use serum_dex::{
    instruction::{cancel_order, new_order, settle_funds, SelfTradeBehavior},
    matching::{OrderType, Side},
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    rent::Rent,
    system_instruction::create_account,
    sysvar::Sysvar,
};
use spl_associated_token_account::get_associated_token_address;
use spl_token::{
    instruction::{initialize_mint, mint_to, transfer, burn},
    state::Account,
    state::Mint,
};

pub struct Processor {}

impl Processor {
    pub fn process_init(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        max_number_of_assets: u32,
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

        let state_size =
            PoolHeader::LEN + max_number_of_assets as usize * instruction::MARKET_DATA_SIZE;

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
                mint_account.clone(),
            ],
            &[&[&pool_seed, &[1]]],
        )?;

        invoke(
            &init_mint,
            &[mint_account.clone(), rent_sysvar_account.clone()],
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
        let order_tracker_account = next_account_info(accounts_iter)?;
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
        let (order_tracker_key, bump) = Pubkey::find_program_address(
            &[&pool_seed, &openorders_account.key.to_bytes()],
            program_id,
        );
        if &order_tracker_key != order_tracker_account.key {
            msg!("Provided order state account does not match the provided OpenOrders account and pool seed.");
            return Err(ProgramError::InvalidArgument);
        }

        let init_pool_account = create_account(
            &payer_account.key,
            &order_tracker_key,
            rent.minimum_balance(OrderTracker::LEN),
            OrderTracker::LEN as u64,
            &program_id,
        );

        invoke_signed(
            &init_pool_account,
            &[
                system_program_account.clone(),
                payer_account.clone(),
                order_tracker_account.clone(),
            ],
            &[&[&pool_seed, &openorders_account.key.to_bytes(), &[bump]]],
        )?;

        Ok(())
    }

    pub fn process_create(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        signal_provider_key: &Pubkey,
        deposit_amounts: Vec<u64>,
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
        let pool_status =
            PoolHeader::unpack_from_slice(&pool_account.try_borrow_data()?[..PoolHeader::LEN])
                .unwrap()
                .status;
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
            let mint_asset_key =
                Account::unpack(&pool_assets_accounts[i as usize].data.borrow())?.mint;
            let pool_asset_key = get_associated_token_address(&pool_key, &mint_asset_key);

            if pool_asset_key != *pool_assets_accounts[i as usize].key {
                msg!("Provided pool asset account is invalid");
                return Err(ProgramError::InvalidArgument);
            }

            // Verify that the first deposit credits more than the min amount of FIDA tokens
            enough_fida = ((&mint_asset_key.to_string()[..] == FIDA_MINT_KEY)
                & (deposit_amounts[i] >= FIDA_MIN_AMOUNT))
                | enough_fida;

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
                mint_address: mint_asset_key,
                amount_in_token: deposit_amounts[i as usize],
            });
        }

        if !enough_fida {
            msg!(
                "Pool should always hold at least {:?} FIDA tokens",
                FIDA_MIN_AMOUNT
            );
            return Err(ProgramError::InvalidArgument);
        }

        // Mint the first pooltoken to the target
        let instruction = mint_to(
            spl_token_account.key,
            &mint_key,
            target_pool_token_account.key,
            &pool_key,
            &[],
            1000000,
        )?;

        invoke_signed(
            &instruction,
            &[
                spl_token_account.clone(),
                mint_account.clone(),
                target_pool_token_account.clone(),
                pool_account.clone(),
            ],
            &[&[&pool_seed]],
        )?;

        // Write state header into data
        let state_header = PoolHeader {
            signal_provider: *signal_provider_key,
            status: PoolStatus::Unlocked,
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
        let pool_mint_key =
            Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();
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
        match pool_header.status {
            PoolStatus::Unlocked => {()}
            _ => {
                match pool_header.status {
                    PoolStatus::Locked | PoolStatus::LockedPendingOrder(_) => {
                        msg!("The signal provider has currently locked the pool. No buy-ins are possible for now.")
                    }
                    PoolStatus::PendingOrder(_) => {
                        msg!("The pool has one or more pending orders. No buy-ins are possible for now. Try again later.")
                    }
                    _ => { unreachable!() }
                };
                return Err(BonfidaBotError::LockedOperation.into())
            }

        };

        // Compute buy-in amount. The effective buy-in amount can be less than the
        // input_token_amount as the source accounts need to satisfy the pool asset ratios
        let mut pool_token_effective_amount = std::u64::MAX;
        for i in 0..nb_assets {
            let source_asset_amount =
                Account::unpack(&source_assets_accounts[i].data.borrow())?.amount;
            pool_token_effective_amount = min(
                source_asset_amount
                    .checked_div(pool_assets[i].amount_in_token)
                    .unwrap_or(std::u64::MAX),
                pool_token_effective_amount,
            );
        }
        pool_token_effective_amount = min(pool_token_amount, pool_token_effective_amount);

        // Execute buy in
        for i in 0..nb_assets {
            let pool_asset_key =
                get_associated_token_address(&pool_key, &pool_assets[i].mint_address);

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
            pool_token_effective_amount,
        )?;

        invoke_signed(
            &instruction,
            &[
                spl_token_account.clone(),
                mint_account.clone(),
                target_pool_token_account.clone(),
                pool_account.clone(),
            ],
            &[&[&pool_seed]],
        )?;

        Ok(())
    }

    pub fn process_create_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        side: Side,
        limit_price: NonZeroU64,
        max_qty: NonZeroU16,
        order_type: OrderType,
        client_id: u64,
        self_trade_behavior: SelfTradeBehavior,
        source_index: usize,
        target_index: usize,
    ) -> ProgramResult {
        // TODO : Enforce one order limit on openorders accounts

        let account_iter = &mut accounts.iter();

        let signal_provider_account = next_account_info(account_iter)?;
        let market = next_account_info(account_iter)?;
        let pool_asset_token_account = next_account_info(account_iter)?;
        let openorders_account = next_account_info(account_iter)?;
        let order_tracker_account = next_account_info(account_iter)?;
        let request_queue = next_account_info(account_iter)?;
        let pool_account = next_account_info(account_iter)?;
        let coin_vault = next_account_info(account_iter)?;
        let pc_vault = next_account_info(account_iter)?;
        let spl_token_program = next_account_info(account_iter)?;
        let rent_sysvar_account = next_account_info(account_iter)?;
        let dex_program_id = next_account_info(account_iter)?;
        let discount_account = next_account_info(account_iter).ok();

        check_pool_key(program_id, pool_account.key, &pool_seed)?;

        check_order_tracker(
            program_id,
            order_tracker_account.key,
            &pool_seed,
            openorders_account.key,
        )?;

        let coin_mint = Pubkey::new(&market.data.borrow()[53..85]);
        let coin_lot_size =
            u64::from_le_bytes(market.data.borrow()[349..357].try_into().ok().unwrap());
        let pc_mint = Pubkey::new(&market.data.borrow()[85..117]);
        let pc_lot_size =
            u64::from_le_bytes(market.data.borrow()[357..365].try_into().ok().unwrap());

        check_open_orders_account(openorders_account, pool_account.key)?;

        let source_account = Account::unpack(&pool_asset_token_account.data.borrow()).or_else(|e|{
            msg!("Invalid pool asset token account provided");
            Err(e)
        })?;
        let source_token_account_key =
            get_associated_token_address(pool_account.key, &source_account.mint);

        if pool_asset_token_account.key != &source_token_account_key {
            msg!("Source token account should be associated to the pool account");
            return Err(ProgramError::InvalidArgument);
        }

        let mut pool_header = PoolHeader::unpack(&pool_account.data.borrow()[..PoolHeader::LEN])?;
        if !signal_provider_account.is_signer {
            msg!("The signal provider's signature is required.");
            return Err(ProgramError::MissingRequiredSignature);
        }
        if signal_provider_account.key != &pool_header.signal_provider {
            msg!("A wrong signal provider account was provided.");
            return Err(ProgramError::MissingRequiredSignature);
        }
        match pool_header.status {
            PoolStatus::Uninitialized => return Err(ProgramError::UninitializedAccount),
            PoolStatus::Unlocked => {
                pool_header.status = PoolStatus::PendingOrder(NonZeroU8::new(1).unwrap())
            }
            PoolStatus::Locked => {
                pool_header.status = PoolStatus::LockedPendingOrder(NonZeroU8::new(1).unwrap())
            }
            PoolStatus::PendingOrder(n) | PoolStatus::LockedPendingOrder(n) => {
                if n.get() == 64 {
                    msg!("Maximum number of pending orders has been reached. Settle or cancel a pending order.");
                    return Err(BonfidaBotError::Overflow.into());
                }
                let pending_orders = NonZeroU8::new(n.get() + 1).unwrap();
                pool_header.status = match pool_header.status {
                    PoolStatus::PendingOrder(_) => PoolStatus::PendingOrder(pending_orders),
                    PoolStatus::LockedPendingOrder(_) => {
                        PoolStatus::LockedPendingOrder(pending_orders)
                    }
                    _ => {
                        unreachable!()
                    }
                }
            }
        };



        let mut source_asset = unpack_unchecked_asset(&pool_account.data.borrow(), source_index)?;
        let mut target_asset = unpack_unchecked_asset(&pool_account.data.borrow(), target_index)?;

        if !source_asset.is_initialized() {
            msg!("The pool has no account at the specificed source index");
            return Err(ProgramError::InvalidArgument);
        }

        if source_asset.mint_address != source_account.mint {
            msg!("Provided coin account does not match the pool source asset");
            return Err(ProgramError::InvalidArgument);
        }

        if &source_account.owner != pool_account.key {
            msg!("Provided coin account should be owned by the pool");
            return Err(ProgramError::InvalidArgument);
        }

        let target_mint = match side {
            Side::Bid => coin_mint,
            Side::Ask => pc_mint,
        };

        if target_asset.is_initialized() {
            if target_asset.mint_address != target_mint {
                msg!("Target asset does not match bid currency");
                return Err(ProgramError::InvalidArgument);
            }
        } else {
            target_asset.mint_address = target_mint;
            pack_asset(
                &mut pool_account.data.borrow_mut(),
                &target_asset,
                target_index,
            )?;
        }

        if source_asset.mint_address
            != match side {
                Side::Bid => pc_mint,
                Side::Ask => coin_mint,
            }
        {
            msg!("Wrong source index provided.");
            return Err(ProgramError::InvalidArgument);
        }

        let cast_value: u128 = source_asset.amount_in_token.into();

        let amount_to_trade_in_token = (cast_value
            .checked_mul(max_qty.get().into())
            .ok_or(BonfidaBotError::Overflow)?
            >> 16) as u64;

        let lots_to_trade = amount_to_trade_in_token
            .checked_div(match side {
                Side::Bid => pc_lot_size,
                Side::Ask => coin_lot_size,
            })
            .ok_or(BonfidaBotError::Overflow)?;

        let expected_target_tokens = match side {
            Side::Bid => lots_to_trade
                .checked_div(limit_price.get())
                .and_then(|n| n.checked_mul(coin_lot_size))
                .ok_or_else(|| {
                    msg!("Limit price caused an overflow. Reduce the size of the order.");
                    BonfidaBotError::Overflow
                })?,
            Side::Ask => lots_to_trade
                .checked_mul(limit_price.get())
                .and_then(|n| n.checked_mul(pc_lot_size))
                .ok_or(ProgramError::InvalidArgument)?,
        };

        source_asset.amount_in_token = source_asset.amount_in_token - amount_to_trade_in_token;

        pack_asset(
            &mut pool_account.data.borrow_mut(),
            &source_asset,
            source_index,
        )?;

        let order_tracker = OrderTracker {
            side,
            source_amount_per_token: amount_to_trade_in_token,
            pending_target_amount: expected_target_tokens,
        };

        order_tracker.pack_into_slice(&mut order_tracker_account.data.borrow_mut());

        let new_order_instruction = new_order(
            market.key,
            openorders_account.key,
            request_queue.key,
            pool_asset_token_account.key,
            pool_account.key,
            coin_vault.key,
            pc_vault.key,
            spl_token_program.key,
            rent_sysvar_account.key,
            discount_account.map(|account| account.key),
            dex_program_id.key,
            side,
            limit_price,
            NonZeroU64::new(lots_to_trade).ok_or(BonfidaBotError::Overflow)?,
            order_type,
            client_id,
            self_trade_behavior,
        )?;

        let mut account_infos = vec![
            market.clone(),
            openorders_account.clone(),
            request_queue.clone(),
            pool_asset_token_account.clone(),
            pool_account.clone(),
            coin_vault.clone(),
            pc_vault.clone(),
            spl_token_program.clone(),
            rent_sysvar_account.clone(),
        ];

        if let Some(account) = discount_account {
            account_infos.push(account.clone());
        }

        invoke_signed(&new_order_instruction, &account_infos, &[&[&pool_seed]])?;

        Ok(())
    }

    pub fn process_settle(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        pc_index: usize,
        coin_index: usize,
    ) -> ProgramResult {
        let account_iter = &mut accounts.iter();
        let market = next_account_info(account_iter)?;
        let openorders_account = next_account_info(account_iter)?;
        let order_tracker_account = next_account_info(account_iter)?;
        let pool_account = next_account_info(account_iter)?;
        let pool_token_mint = next_account_info(account_iter)?;
        let coin_vault = next_account_info(account_iter)?;
        let pc_vault = next_account_info(account_iter)?;
        let pool_coin_wallet = next_account_info(account_iter)?;
        let pool_pc_wallet = next_account_info(account_iter)?;
        let vault_signer = next_account_info(account_iter)?;
        let spl_token_program = next_account_info(account_iter)?;
        let dex_program = next_account_info(account_iter)?;

        let discount_account = next_account_info(account_iter).ok();

        check_pool_key(program_id, pool_account.key, &pool_seed)?;

        check_order_tracker(
            program_id,
            order_tracker_account.key,
            &pool_seed,
            openorders_account.key,
        )?;

        let mut order_tracker = OrderTracker::unpack(&order_tracker_account.data.borrow())
            .map_err(|e| {
                msg!("Provided order is empty");
                e
            })?;

        let coin_mint = Pubkey::new(&market.data.borrow()[53..85]);
        let pc_mint = Pubkey::new(&market.data.borrow()[85..117]);

        let pool_coin_account_key = get_associated_token_address(pool_account.key, &coin_mint);
        let pool_pc_account_key = get_associated_token_address(pool_account.key, &pc_mint);
        let pool_mint_key =
            Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();

        if &pool_mint_key != pool_token_mint.key {
            msg!("Provided pool mint account is invalid.");
            return Err(ProgramError::InvalidArgument);
        }

        let pool_mint_account = Mint::unpack(&pool_token_mint.data.borrow())?;

        if &pool_coin_account_key != pool_coin_wallet.key {
            msg!("Provided pool coin account does not match the pool coin asset");
            return Err(ProgramError::InvalidArgument);
        }
        if &pool_pc_account_key != pool_pc_wallet.key {
            msg!("Provided pool pc account does not match the pool pc asset");
            return Err(ProgramError::InvalidArgument);
        }

        let pool_coin_account = Account::unpack(&pool_coin_wallet.data.borrow())?;
        let pool_pc_account = Account::unpack(&pool_pc_wallet.data.borrow())?;

        let mut pool_coin_asset = unpack_unchecked_asset(&pool_account.data.borrow(), coin_index)?;
        let mut pool_pc_asset = unpack_unchecked_asset(&pool_account.data.borrow(), pc_index)?;

        if &pool_coin_account.owner != pool_account.key {
            msg!("Pool should own the provided coin account");
            return Err(ProgramError::InvalidArgument);
        }

        if &pool_pc_account.owner != pool_account.key {
            msg!("Pool should own the provided price coin account");
            return Err(ProgramError::InvalidArgument);
        }

        if pool_coin_asset.is_initialized() {
            if pool_coin_asset.mint_address != coin_mint {
                msg!("Coin asset does not match market coin token");
                return Err(ProgramError::InvalidArgument);
            }
        } else {
            pool_coin_asset.mint_address = coin_mint
        }

        if pool_pc_asset.is_initialized() {
            if pool_pc_asset.mint_address != pc_mint {
                msg!("Coin asset does not match market pc token");
                return Err(ProgramError::InvalidArgument);
            }
        } else {
            pool_pc_asset.mint_address = pc_mint
        }

        // TODO : check offsets
        let openorders_account_owner = Pubkey::new(&openorders_account.data.borrow()[40..72]);

        if &openorders_account_owner != pool_account.key {
            msg!("The pool account should own the open orders account");
        }

        let openorders_free_pc = openorders_account
            .data
            .borrow()
            .get(88..96)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;
        let openorders_free_coin = openorders_account
            .data
            .borrow()
            .get(72..80)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;

        let openorders_total_pc = openorders_account
            .data
            .borrow()
            .get(96..108)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;
        let openorders_total_coin = openorders_account
            .data
            .borrow()
            .get(80..88)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;
        
        if (openorders_free_pc == openorders_total_pc) & (openorders_free_coin == openorders_total_coin) {
            // This means the order can be entirely settled.
            let mut pool_header = PoolHeader::unpack(&pool_account.data.borrow())?;
            pool_header.status = match pool_header.status {
                PoolStatus::PendingOrder(n) |
                PoolStatus::LockedPendingOrder(n) => {
                    if n.get() == 1{
                        match pool_header.status {
                            PoolStatus::PendingOrder(_) => {PoolStatus::Unlocked}
                            PoolStatus::LockedPendingOrder(_) => {PoolStatus::Locked}
                            _ => {unreachable!()}
                        }
                    } else {
                        let pending_orders = NonZeroU8::new(n.get() - 1).unwrap();
                        match pool_header.status {
                            PoolStatus::PendingOrder(_) => {PoolStatus::PendingOrder(pending_orders)}
                            PoolStatus::LockedPendingOrder(_) => {PoolStatus::LockedPendingOrder(pending_orders)}
                            _ => {unreachable!()}
                        }
                    }
                }
                _ => { return Err(ProgramError::InvalidAccountData) }
            }

        }

        // TODO: Think about this attack vector when operations are too small to be picked up by this division
        let (free_source_amount, total_source_amount, free_target_amount) = match order_tracker.side
        {
            Side::Bid => (
                openorders_free_pc,
                openorders_total_pc,
                openorders_free_coin,
            ),
            Side::Ask => (
                openorders_free_coin,
                openorders_total_coin,
                openorders_free_coin,
            ),
        };
        let source_proportion_of_order = ((free_source_amount as u128) << 64)
            .checked_div(total_source_amount as u128)
            .ok_or(ProgramError::InvalidAccountData)?
            as u64;

        let target_proportion_of_order = ((free_target_amount as u128) << 64)
            .checked_div(order_tracker.pending_target_amount as u128)
            .ok_or(ProgramError::InvalidAccountData)?
            as u64;

        if (source_proportion_of_order == 0) & (target_proportion_of_order == 0) {
            msg!("Settle operation is too small");
            return Err(BonfidaBotError::Overflow.into());
        }

        order_tracker.pending_target_amount =
            order_tracker.pending_target_amount - free_target_amount;
        order_tracker.source_amount_per_token = (((std::u64::MAX - source_proportion_of_order) as u128)
            .checked_mul(order_tracker.source_amount_per_token as u128)
            .ok_or(BonfidaBotError::Overflow)?
            >> 64) as u64;

        let total_coin_assets = pool_coin_account.amount + openorders_free_coin;
        let total_pc_assets = pool_pc_account.amount + openorders_free_pc;

        pool_coin_asset.amount_in_token = total_coin_assets
            .checked_div(pool_mint_account.supply)
            .ok_or(BonfidaBotError::Overflow)?;
        pool_pc_asset.amount_in_token = total_pc_assets
            .checked_div(pool_mint_account.supply)
            .ok_or(BonfidaBotError::Overflow)?;

        order_tracker.pack_into_slice(&mut order_tracker_account.data.borrow_mut());
        pack_asset(
            &mut pool_account.data.borrow_mut(),
            &pool_coin_asset,
            coin_index,
        )?;
        pack_asset(
            &mut pool_account.data.borrow_mut(),
            &pool_pc_asset,
            pc_index,
        )?;

        let instruction = settle_funds(
            dex_program.key,
            market.key,
            spl_token_program.key,
            openorders_account.key,
            pool_account.key,
            coin_vault.key,
            pool_coin_wallet.key,
            pc_vault.key,
            pool_pc_wallet.key,
            discount_account.map(|a| a.key),
            vault_signer.key,
        )?;

        let mut accounts = vec![
            dex_program.clone(),
            market.clone(),
            openorders_account.clone(),
            pool_account.clone(),
            coin_vault.clone(),
            pc_vault.clone(),
            pool_coin_wallet.clone(),
            pool_pc_wallet.clone(),
            vault_signer.clone(),
            spl_token_program.clone(),
        ];

        if let Some(a) = discount_account {
            accounts.push(a.clone())
        }

        invoke_signed(&instruction, &accounts, &[&[&pool_seed]])?;

        Ok(())
    }

    pub fn process_cancel(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        side: Side,
        order_id: u128,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let signal_provider = next_account_info(accounts_iter)?;
        let market = next_account_info(accounts_iter)?;
        let openorders_account = next_account_info(accounts_iter)?;
        let request_queue = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;
        let dex_program = next_account_info(accounts_iter)?;

        check_pool_key(program_id, pool_account.key, &pool_seed)?;
        check_open_orders_account(openorders_account, pool_account.key)?;

        let pool_header = PoolHeader::unpack(&pool_account.data.borrow())?;
        check_signal_provider(&pool_header, signal_provider, true)?;

        let instruction = cancel_order(
            program_id,
            market.key,
            openorders_account.key,
            pool_account.key,
            request_queue.key,
            side,
            order_id,
            [0u64; 4],
            0,
        )?;

        invoke_signed(
            &instruction,
            &vec![
                dex_program.clone(),
                openorders_account.clone(),
                request_queue.clone(),
                pool_account.clone(),
            ],
            &[&[&pool_seed]],
        )?;

        Ok(())
    }

    pub fn process_redeem(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        // The amount of pooltokens wished to be redeemed
        pool_token_amount: u64,
    ) -> ProgramResult {
        let accounts_iter = &mut accounts.iter();

        let spl_token_account = next_account_info(accounts_iter)?;
        let mint_account = next_account_info(accounts_iter)?;
        let source_pool_token_owner_account = next_account_info(accounts_iter)?;
        let source_pool_token_account = next_account_info(accounts_iter)?;
        let pool_account = next_account_info(accounts_iter)?;

        let pool_header = PoolHeader::unpack(&pool_account.data.borrow()[..PoolHeader::LEN])?;
        let pool_assets = unpack_assets(&pool_account.data.borrow()[PoolHeader::LEN..])?;
        let nb_assets = pool_assets.len();

        let mut pool_assets_accounts: Vec<&AccountInfo> = vec![];
        let mut target_assets_accounts: Vec<&AccountInfo> = vec![];
        for _ in 0..nb_assets {
            pool_assets_accounts.push(next_account_info(accounts_iter)?)
        }
        for _ in 0..nb_assets {
            target_assets_accounts.push(next_account_info(accounts_iter)?)
        }

        // Safety verifications
        check_pool_key(&program_id, &pool_account.key, &pool_seed)?;
        let pool_mint_key = Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();
        if pool_mint_key != *mint_account.key {
            msg!("Provided mint account is invalid");
            return Err(ProgramError::InvalidArgument);
        }
        if !source_pool_token_owner_account.is_signer {
            msg!("Source pooltoken account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }
        if *pool_account.owner != *program_id {
            msg!("Program should own pool account");
            return Err(ProgramError::InvalidArgument);
        }
        match pool_header.status {
            PoolStatus::PendingOrder(_) | PoolStatus::LockedPendingOrder(_) => {
                msg!("The pool has one or more pending orders. No buy-outs are possible for now. Try again later.");
                return Err(BonfidaBotError::LockedOperation.into())
            },
            _ => {()}
        };
        
        // Execute buy out
        for i in 0..nb_assets {
            let pool_asset_key =
            get_associated_token_address(&pool_account.key, &pool_assets[i].mint_address);
            
            if pool_asset_key != *pool_assets_accounts[i as usize].key {
                msg!("Provided pool asset account is invalid");
                return Err(ProgramError::InvalidArgument);
            }
            
            let amount = pool_token_amount
            .checked_mul(pool_assets[i].amount_in_token)
            .ok_or(BonfidaBotError::Overflow)?;
            if amount == 0 {
                break;
            }
            let instruction = transfer(
                spl_token_account.key,
                pool_assets_accounts[i].key,
                target_assets_accounts[i].key,
                pool_account.key,
                &[],
                amount,
            )?;
            invoke(
                &instruction,
                &[
                    pool_assets_accounts[i].clone(),
                    target_assets_accounts[i].clone(),
                    spl_token_account.clone(),
                    pool_account.clone(),
                    ],
                )?;
            }

        // Burn the redeemed pooltokens
        let instruction = burn(
            spl_token_account.key,
            &source_pool_token_account.key,
            mint_account.key,
            &pool_account.key,
            &[],
            pool_token_amount,
        )?;

        invoke_signed(
            &instruction,
            &[
                spl_token_account.clone(),
                source_pool_token_account.clone(),
                mint_account.clone(),
                source_pool_token_owner_account.clone(),
            ],
            &[&[&pool_seed]],
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
                max_number_of_assets,
            } => {
                msg!("Instruction: Init");
                Self::process_init(program_id, accounts, pool_seed, max_number_of_assets)
            }
            PoolInstruction::InitOrderTracker { pool_seed } => {
                msg!("Instruction: Init Order Tracker");
                Self::process_init_order_tracker(program_id, accounts, pool_seed)
            }
            PoolInstruction::Create {
                pool_seed,
                signal_provider_key,
                deposit_amounts,
            } => {
                msg!("Instruction: Create Pool");
                Self::process_create(
                    program_id,
                    accounts,
                    pool_seed,
                    &signal_provider_key,
                    deposit_amounts,
                )
            }
            PoolInstruction::Deposit {
                pool_seed,
                pool_token_amount,
            } => {
                msg!("Instruction: Deposit into Pool");
                Self::process_deposit(program_id, accounts, pool_seed, pool_token_amount)
            }
            PoolInstruction::CreateOrder {
                pool_seed,
                side,
                limit_price,
                max_qty,
                order_type,
                client_id,
                self_trade_behavior,
                source_index,
                target_index
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
                    source_index as usize,
                    target_index as usize,
                )
            }
            PoolInstruction::SettleFunds {
                pool_seed,
                pc_index,
                coin_index,
            } => {
                msg!("Instruction: Settle funds for Pool");
                Self::process_settle(
                    program_id,
                    accounts,
                    pool_seed,
                    pc_index as usize,
                    coin_index as usize,
                )
            }
            PoolInstruction::CancelOrder {
                pool_seed,
                side,
                order_id,
            } => {
                msg!("Instruction: Cancel Order for Pool");
                Self::process_cancel(program_id, accounts, pool_seed, side, order_id)
            }
            PoolInstruction::Redeem {
                pool_seed,
                pool_token_amount,
            } => {
                msg!("Instruction: Redeem out of Pool");
                Self::process_redeem(program_id, accounts, pool_seed, pool_token_amount)
            }
        }
    }
}
