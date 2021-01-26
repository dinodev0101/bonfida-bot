use std::convert::{TryFrom, TryInto};

use solana_program::{
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};

#[derive(Debug, PartialEq)]
pub struct PoolAsset {
    pub mint_address: Pubkey,
    pub amount_in_token: u64
}
#[derive(Debug, PartialEq)]
pub enum PoolStatus {
    UNLOCKED,
    LOCKED,
}

#[derive(Debug, PartialEq)]
pub struct PoolHeader {
    pub signal_provider: Pubkey,
    pub is_initialized: bool,
    pub status: PoolStatus,
}

impl Sealed for PoolHeader {}

impl Pack for PoolHeader {
    const LEN: usize = 33;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let signal_provider_bytes = self.signal_provider.to_bytes();
        for i in 0..32 {
            target[i] = signal_provider_bytes[i];
        }
        target[32] = self.is_initialized as u8;
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let signal_provider = Pubkey::new(&src[..32]);
        let is_initialized = (src[32] & 0xf) == 1;
        let status = match src[32] >> 4 {
            0 => {PoolStatus::UNLOCKED},
            1 => {PoolStatus::LOCKED},
            _ => return Err(ProgramError::InvalidAccountData)
        };
        Ok(Self {
            signal_provider,
            is_initialized,
            status
        })
    }

    fn unpack(input: &[u8]) -> Result<Self, ProgramError>
    where
        Self: IsInitialized,
    {
        let value = Self::unpack_unchecked(input)?;
        if value.is_initialized() {
            Ok(value)
        } else {
            Err(ProgramError::UninitializedAccount)
        }
    }

    fn unpack_unchecked(input: &[u8]) -> Result<Self, ProgramError> {
        if input.len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(Self::unpack_from_slice(input)?)
    }

    fn pack(src: Self, dst: &mut [u8]) -> Result<(), ProgramError> {
        if dst.len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        src.pack_into_slice(dst);
        Ok(())
    }
}

impl IsInitialized for PoolHeader {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}


impl Sealed for PoolAsset {}

impl Pack for PoolAsset {
    const LEN: usize = 40;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let mint_address_bytes = self.mint_address.to_bytes();
        let amount_bytes = self.amount_in_token.to_le_bytes();
        for i in 0..32 {
            target[i] = mint_address_bytes[i]
        }

        for i in 32..40 {
            target[i] = amount_bytes[i-32]
        }
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let mint_address = Pubkey::new(&src[..32]);
        let amount_in_token = u64::from_le_bytes(src[32..40].try_into().unwrap());
        Ok(Self {
            mint_address,
            amount_in_token
        })
    }
}

fn unpack_assets(input: &[u8]) -> Result<Vec<PoolAsset>, ProgramError>{
    let number_of_assets = input.len() / PoolAsset::LEN;
    let mut output: Vec<PoolAsset> = Vec::with_capacity(number_of_assets);
    let mut offset = 0;
    for _ in 0..number_of_assets{
        output.push(PoolAsset::unpack_from_slice(
            &input[offset..offset + PoolAsset::LEN],
        )?);
        offset += PoolAsset::LEN;
    }
    Ok(output)
}


#[cfg(test)]
mod tests {
    use super::{PoolHeader, PoolStatus};
    use solana_program::{program_pack::Pack, pubkey::Pubkey};

    #[test]
    fn test_state_packing() {
        let header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            is_initialized: true,
            status: PoolStatus::UNLOCKED
        };

        let state_size = PoolHeader::LEN;
        let mut state_array = [0u8; 33];
        header_state.pack_into_slice(&mut state_array[..state_size]);

        let packed = Vec::from(state_array);
        let mut expected = Vec::with_capacity(state_size);
        expected.extend_from_slice(&header_state.signal_provider.to_bytes());
        expected.extend_from_slice(&[header_state.is_initialized as u8]);

        assert_eq!(expected, packed);
        assert_eq!(packed.len(), state_size);
        let unpacked_header =
        PoolHeader::unpack(&packed[..PoolHeader::LEN]).unwrap();
        assert_eq!(unpacked_header, header_state);
    }
}
