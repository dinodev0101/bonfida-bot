use std::{
    cmp::min,
    convert::TryInto,
    num::{NonZeroU16, NonZeroU64, NonZeroU8},
};

use crate::{error::BonfidaBotError, instruction::PoolInstruction, state::{FIDA_MINT_KEY, FIDA_MIN_AMOUNT, PUBKEY_LENGTH, PoolAsset, PoolHeader, PoolStatus, get_asset_slice, pack_markets, unpack_assets, unpack_market, unpack_unchecked_asset}, utils::{check_pool_key, check_signal_provider, fill_slice}};
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
    instruction::{burn, initialize_mint, mint_to, transfer},
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
        number_of_markets: u16,
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

        let state_size = PoolHeader::LEN
            + PUBKEY_LENGTH * (number_of_markets as usize)
            + max_number_of_assets as usize * PoolAsset::LEN;

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

    pub fn process_create(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        pool_seed: [u8; 32],
        serum_program_id: &Pubkey,
        signal_provider_key: &Pubkey,
        deposit_amounts: Vec<u64>,
        markets: Vec<Pubkey>,
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
        if markets.len() >> 16 != 0 {
            msg!("Number of given markets is too high.");
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
            serum_program_id: *serum_program_id,
            signal_provider: *signal_provider_key,
            status: PoolStatus::Unlocked,
            number_of_markets: markets.len() as u16,
        };
        let mut data = pool_account.data.borrow_mut();
        state_header.pack_into_slice(&mut data);

        // Write the authorized markets to the account data
        pack_markets(&mut data[PoolHeader::LEN..], &markets)?;

        // Write the assets into the account data
        let mut offset = PoolHeader::LEN + PUBKEY_LENGTH * markets.len();
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
        let asset_offset = PoolHeader::LEN + PUBKEY_LENGTH * pool_header.number_of_markets as usize;
        let pool_assets = unpack_assets(&pool_account.data.borrow()[asset_offset..])?;
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
        if pool_token_amount < 1_000 {
            msg!("Provided pool token amount isn't sufficient.");
            return Err(ProgramError::InvalidArgument);
        }
        if pool_key != *pool_account.key {
            msg!("Provided pool account doesn't match the provided pool seed.");
            return Err(ProgramError::InvalidArgument);
        }
        if pool_mint_key != *mint_account.key {
            msg!("Provided mint account is invalid.");
            return Err(ProgramError::InvalidArgument);
        }
        if !source_owner_account.is_signer {
            msg!("Source token account owner should be a signer.");
            return Err(ProgramError::InvalidArgument);
        }
        if *pool_account.owner != *program_id {
            msg!("Program should own pool account.");
            return Err(ProgramError::InvalidArgument);
        }
        match pool_header.status {
            PoolStatus::Unlocked => (),
            _ => {
                match pool_header.status {
                    PoolStatus::Locked | PoolStatus::LockedPendingOrder(_) => {
                        msg!("The signal provider has currently locked the pool. No buy-ins are possible for now.")
                    }
                    PoolStatus::PendingOrder(_) => {
                        msg!("The pool has one or more pending orders. No buy-ins are possible for now. Try again later.")
                    }
                    _ => {
                        unreachable!()
                    }
                };
                return Err(BonfidaBotError::LockedOperation.into());
            }
        };

        let total_pooltokens = Mint::unpack(&mint_account.data.borrow())?.supply;
        let mut pool_asset_amounts = Vec::with_capacity(nb_assets);
        // Compute buy-in amount. The effective buy-in amount can be less than the
        // input_token_amount as the source accounts need to satisfy the pool asset ratios
        let mut pool_token_effective_amount = std::u64::MAX;
        for i in 0..nb_assets {
            let pool_asset_amount = Account::unpack(&pool_assets_accounts[i].data.borrow())?.amount;
            pool_asset_amounts.push(pool_asset_amount);

            let source_asset_amount =
                Account::unpack(&source_assets_accounts[i].data.borrow())?.amount;
            pool_token_effective_amount = min(
                ((source_asset_amount as u128) * (total_pooltokens as u128))
                    .checked_div(pool_asset_amount as u128)
                    .unwrap_or(std::u64::MAX.into()) as u64,
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

            let amount = ((pool_token_effective_amount as u128) * (pool_asset_amounts[i] as u128))
                / (total_pooltokens as u128);
            if amount == 0 {
                continue;
            }
            let instruction = transfer(
                spl_token_account.key,
                source_assets_accounts[i].key,
                pool_assets_accounts[i].key,
                source_owner_account.key,
                &[],
                amount as u64,
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
        max_ratio_of_pool_to_sell_to_another_fellow_trader: NonZeroU16,
        order_type: OrderType,
        market_index: u16,
        coin_lot_size: u64,
        pc_lot_size: u64,
        target_mint: Pubkey,
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
        let request_queue = next_account_info(account_iter)?;
        let pool_account = next_account_info(account_iter)?;
        let coin_vault = next_account_info(account_iter)?;
        let pc_vault = next_account_info(account_iter)?;
        let spl_token_program = next_account_info(account_iter)?;
        let rent_sysvar_account = next_account_info(account_iter)?;
        let dex_program = next_account_info(account_iter)?;
        let discount_account = next_account_info(account_iter).ok();

        check_pool_key(program_id, pool_account.key, &pool_seed)?;

        let source_account =
            Account::unpack(&pool_asset_token_account.data.borrow()).or_else(|e| {
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
        if &pool_header.serum_program_id != dex_program.key {
            msg!("The provided serum program account is invalid for this pool.");
            return Err(ProgramError::InvalidArgument);
        }
        if !signal_provider_account.is_signer {
            msg!("The signal provider's signature is required.");
            return Err(ProgramError::MissingRequiredSignature);
        }
        if signal_provider_account.key != &pool_header.signal_provider {
            msg!("A wrong signal provider account was provided.");
            return Err(ProgramError::MissingRequiredSignature);
        }
        if market.key
            != &unpack_market(&pool_account.data.borrow()[PoolHeader::LEN..], market_index)
        {
            msg!("The given market account is not authorized.");
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
        pool_header.pack_into_slice(&mut pool_account.data.borrow_mut()[..PoolHeader::LEN]);

        let asset_offset = PoolHeader::LEN + PUBKEY_LENGTH * pool_header.number_of_markets as usize;
        let source_asset =
            unpack_unchecked_asset(&pool_account.data.borrow()[asset_offset..], source_index)?;
        let mut target_asset =
            unpack_unchecked_asset(&pool_account.data.borrow()[asset_offset..], target_index)?;

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

        if target_asset.is_initialized() {
            if target_asset.mint_address != target_mint {
                msg!("Target asset does not match bid currency");
                return Err(ProgramError::InvalidArgument);
            }
        } else {
            target_asset.mint_address = target_mint;
            &target_asset.pack_into_slice(get_asset_slice(
                &mut pool_account.data.borrow_mut()[asset_offset..],
                target_index,
            )?);
        }

        let pool_asset_amount = Account::unpack(&pool_asset_token_account.data.borrow())?.amount;

        let amount_to_trade = (((pool_asset_amount as u128)
            * (max_ratio_of_pool_to_sell_to_another_fellow_trader.get() as u128))
            >> 16) as u64;

        let lots_to_trade = amount_to_trade
            .checked_div(match side {
                Side::Bid => pc_lot_size,
                Side::Ask => coin_lot_size,
            })
            .ok_or(BonfidaBotError::Overflow)?;

        if pool_asset_amount == amount_to_trade {
            // If order empties a pool asset, reset it
            fill_slice(get_asset_slice(
                &mut pool_account.data.borrow_mut()[asset_offset..],
                source_index,
            )?, 0u8);
        }

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
            dex_program.key,
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

        let mut pool_header = PoolHeader::unpack(&pool_account.data.borrow()[..PoolHeader::LEN])?;

        let asset_offset = PoolHeader::LEN + PUBKEY_LENGTH * pool_header.number_of_markets as usize;
        let mut pool_coin_asset =
            unpack_unchecked_asset(&pool_account.data.borrow()[asset_offset..], coin_index)?;
        let mut pool_pc_asset =
            unpack_unchecked_asset(&pool_account.data.borrow()[asset_offset..], pc_index)?;

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

        let openorders_free_pc = openorders_account
            .data
            .borrow()
            .get(93..101)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;
        let openorders_free_coin = openorders_account
            .data
            .borrow()
            .get(77..85)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;

        let openorders_total_pc = openorders_account
            .data
            .borrow()
            .get(101..109)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;
        let openorders_total_coin = openorders_account
            .data
            .borrow()
            .get(85..93)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(ProgramError::InvalidAccountData)?;

        if (openorders_free_pc == openorders_total_pc)
            && (openorders_free_coin == openorders_total_coin)
        {
            // This means the order can be entirely settled.
            pool_header.status = match pool_header.status {
                PoolStatus::PendingOrder(n) | PoolStatus::LockedPendingOrder(n) => {
                    if n.get() == 1 {
                        match pool_header.status {
                            PoolStatus::PendingOrder(_) => PoolStatus::Unlocked,
                            PoolStatus::LockedPendingOrder(_) => PoolStatus::Locked,
                            _ => {
                                unreachable!()
                            }
                        }
                    } else {
                        let pending_orders = NonZeroU8::new(n.get() - 1).unwrap();
                        match pool_header.status {
                            PoolStatus::PendingOrder(_) => PoolStatus::PendingOrder(pending_orders),
                            PoolStatus::LockedPendingOrder(_) => {
                                PoolStatus::LockedPendingOrder(pending_orders)
                            }
                            _ => {
                                unreachable!()
                            }
                        }
                    }
                }
                _ => return Err(ProgramError::InvalidAccountData),
            }
        }
        pool_header.pack_into_slice(&mut pool_account.data.borrow_mut()[..PoolHeader::LEN]);

        if (openorders_free_pc == 0) & (openorders_free_coin == 0) {
            msg!("No funds to settle.");
            return Err(BonfidaBotError::Overflow.into());
        }

        &pool_coin_asset.pack_into_slice( get_asset_slice(
            &mut pool_account.data.borrow_mut()[asset_offset..],
            coin_index,
        )?);
        &pool_pc_asset.pack_into_slice( get_asset_slice(
            &mut pool_account.data.borrow_mut()[asset_offset..],
            pc_index,
        )?);

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

        let pool_header = PoolHeader::unpack(&pool_account.data.borrow()[..PoolHeader::LEN])?;
        check_signal_provider(&pool_header, signal_provider, true)?;

        let instruction = cancel_order(
            &dex_program.key,
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
                market.clone(),
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
        let asset_offset = PoolHeader::LEN + PUBKEY_LENGTH * pool_header.number_of_markets as usize;
        let pool_assets = unpack_assets(&pool_account.data.borrow()[asset_offset..])?;
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
        let pool_mint_key =
            Pubkey::create_program_address(&[&pool_seed, &[1]], &program_id).unwrap();
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
                return Err(BonfidaBotError::LockedOperation.into());
            }
            _ => (),
        };

        let total_pooltokens = Mint::unpack(&mint_account.data.borrow())?.supply;

        // Execute buy out
        for i in 0..nb_assets {
            let pool_asset_key =
                get_associated_token_address(&pool_account.key, &pool_assets[i].mint_address);

            if pool_asset_key != *pool_assets_accounts[i as usize].key {
                msg!("Provided pool asset account is invalid");
                return Err(ProgramError::InvalidArgument);
            }

            let pool_asset_amount = Account::unpack(&pool_assets_accounts[i].data.borrow())?.amount;

            let amount: u64 = (((pool_token_amount as u128) * (pool_asset_amount as u128))
                / (total_pooltokens as u128))
                .try_into()
                .map_err(|_| BonfidaBotError::Overflow)?;

            if amount == 0 {
                continue;
            }
            let instruction = transfer(
                spl_token_account.key,
                pool_assets_accounts[i].key,
                target_assets_accounts[i].key,
                pool_account.key,
                &[],
                amount,
            )?;
            invoke_signed(
                &instruction,
                &[
                    spl_token_account.clone(),
                    pool_assets_accounts[i].clone(),
                    target_assets_accounts[i].clone(),
                    pool_account.clone(),
                ],
                &[&[&pool_seed]],
            )?;
        }

        // Burn the redeemed pooltokens
        let instruction = burn(
            spl_token_account.key,
            &source_pool_token_account.key,
            mint_account.key,
            &source_pool_token_owner_account.key,
            &[],
            pool_token_amount,
        )?;

        invoke(
            &instruction,
            &[
                spl_token_account.clone(),
                source_pool_token_account.clone(),
                mint_account.clone(),
                source_pool_token_owner_account.clone(),
            ],
        )?;

        if pool_token_amount == total_pooltokens {
            // Reset the pool data
            fill_slice(&mut pool_account.data.borrow_mut(), 0u8);
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
                max_number_of_assets,
                number_of_markets,
            } => {
                msg!("Instruction: Init");
                Self::process_init(
                    program_id,
                    accounts,
                    pool_seed,
                    max_number_of_assets,
                    number_of_markets,
                )
            }
            PoolInstruction::Create {
                pool_seed,
                serum_program_id,
                signal_provider_key,
                deposit_amounts,
                markets,
            } => {
                msg!("Instruction: Create Pool");
                Self::process_create(
                    program_id,
                    accounts,
                    pool_seed,
                    &serum_program_id,
                    &signal_provider_key,
                    deposit_amounts,
                    markets,
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
                ratio_of_pool_assets_to_trade,
                order_type,
                client_id,
                self_trade_behavior,
                source_index,
                target_index,
                market_index,
                coin_lot_size,
                pc_lot_size,
                target_mint,
            } => {
                msg!("Instruction: Create Order for Pool");
                Self::process_create_order(
                    program_id,
                    accounts,
                    pool_seed,
                    side,
                    limit_price,
                    ratio_of_pool_assets_to_trade,
                    order_type,
                    market_index,
                    coin_lot_size,
                    pc_lot_size,
                    target_mint,
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
