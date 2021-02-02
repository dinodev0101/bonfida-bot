use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, msg, program_error::ProgramError,
    pubkey::Pubkey,
};

use crate::state::PoolHeader;

pub fn check_pool_key(program_id: &Pubkey, key: &Pubkey, pool_seed: &[u8; 32]) -> ProgramResult {
    let expected_key = Pubkey::create_program_address(&[pool_seed], program_id)?;

    if &expected_key != key {
        msg!("Provided pool account does not match the provided pool seed");
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}

pub fn check_open_orders_account(
    openorders_account: &AccountInfo,
    pool_key: &Pubkey,
) -> ProgramResult {
    // TODO: Check offsets
    let owner = Pubkey::new(&openorders_account.data.borrow()[40..72]);
    if &owner != pool_key {
        msg!("The pool account should own the open orders account");
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}

pub fn check_signal_provider(
    pool_header: &PoolHeader,
    signal_provider_account: &AccountInfo,
    is_signer: bool,
) -> ProgramResult {
    if &pool_header.signal_provider != signal_provider_account.key {
        msg!("A wrong signal provider account was provided.");
        return Err(ProgramError::MissingRequiredSignature);
    }
    if is_signer & !signal_provider_account.is_signer {
        msg!("The signal provider's signature is required.");
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

pub fn check_order_tracker(
    program_id: &Pubkey,
    order_tracker_key: &Pubkey,
    pool_seed: &[u8; 32],
    openorders_account_key: &Pubkey,
) -> ProgramResult {
    let (order_state_key, _) =
        Pubkey::find_program_address(&[pool_seed, &openorders_account_key.to_bytes()], program_id);
    if &order_state_key != order_tracker_key {
        msg!("Provided order state account does not match the provided OpenOrders account and pool seed.");
        return Err(ProgramError::InvalidArgument);
    }

    Ok(())
}
