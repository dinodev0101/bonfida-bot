use std::convert::TryInto;

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
