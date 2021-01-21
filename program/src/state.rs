use solana_program::{
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};

#[derive(Debug, PartialEq)]
pub struct PoolMarket {
    pub release_height: u64,
    pub amount: u64,
}

#[derive(Debug, PartialEq)]
pub struct PoolHeader {
    pub signal_provider: Pubkey,
    pub is_initialized: bool
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
        let is_initialized = src[32] == 1;
        Ok(Self {
            signal_provider,
            is_initialized,
        })
    }
}

impl IsInitialized for PoolHeader {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

#[cfg(test)]
mod tests {
    use super::PoolHeader;
    use solana_program::{program_pack::Pack, pubkey::Pubkey};

    #[test]
    fn test_state_packing() {
        let header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            is_initialized: true,
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
