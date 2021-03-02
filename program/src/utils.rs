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

pub fn fill_slice(target: &mut [u8], val: u8) {
    for i in 0..target.len() {
        target[i] = val;
    }
}

pub fn pow_fixedpoint_u16(x: u32, n: u64) -> u32 {
    if n == 1{
        x
    } else {
        let q = n >> 1;
        if q == 0 {
            return x
        }
        let p = pow_fixedpoint_u16(x, n >> 1);
        let sq = (p * p) >> 16;
        if n & 1 == 1 {
            (sq * x) >> 16
        } else {
            sq
        }
    }
}

#[cfg(test)]
mod tests {
    use super::pow_fixedpoint_u16;

    #[test]
    fn test_exp(){
        let half:u16 = 1<<15;
        for i in 1..16 {
            assert_eq!(pow_fixedpoint_u16(half as u32, i), 1<<(16 - i));
        }
    }
}
