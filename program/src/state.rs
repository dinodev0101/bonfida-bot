use serum_dex::matching::Side;
use solana_program::{
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};
use std::{convert::TryInto, num::NonZeroU8};

pub const FIDA_MIN_AMOUNT: u64 = 1000000;
pub const FIDA_MINT_KEY: &str = "EchesyfXePKdLtoiZSL8pBe8Myagyy8ZRqsACNCFGnvp";

#[derive(Debug, PartialEq)]
pub struct PoolAsset {
    pub mint_address: Pubkey,
    pub amount_in_token: u64,
}
#[derive(Debug, PartialEq)]
pub enum PoolStatus {
    Uninitialized,
    Unlocked,
    Locked,
    /// Maximum number of pending orders is 64, minimum is 1.
    PendingOrder(NonZeroU8),
    LockedPendingOrder(NonZeroU8),
}

#[derive(Debug, PartialEq)]
pub struct PoolHeader {
    pub signal_provider: Pubkey,
    pub status: PoolStatus,
}

const STATUS_PENDING_ORDER_FLAG: u8 = 1 << 6;
const STATUS_PENDING_ORDER_MASK: u8 = 0x3f;
const STATUS_LOCKED_FLAG: u8 = 2 << 6;
const STATUS_UNLOCKED_FLAG: u8 = STATUS_PENDING_ORDER_MASK;

impl Sealed for PoolHeader {}

impl Pack for PoolHeader {
    const LEN: usize = 33;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let signal_provider_bytes = self.signal_provider.to_bytes();
        for i in 0..32 {
            target[i] = signal_provider_bytes[i];
        }
        target[32] = match self.status {
            PoolStatus::Uninitialized => 0,
            PoolStatus::Unlocked => STATUS_UNLOCKED_FLAG,
            PoolStatus::Locked => STATUS_LOCKED_FLAG,
            PoolStatus::PendingOrder(n) => {
                STATUS_PENDING_ORDER_FLAG | (STATUS_PENDING_ORDER_MASK & (n.get() - 1))
            }
            PoolStatus::LockedPendingOrder(n) => {
                STATUS_LOCKED_FLAG
                    | STATUS_PENDING_ORDER_FLAG
                    | (STATUS_PENDING_ORDER_MASK & (n.get() - 1))
            }
        }
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let signal_provider = Pubkey::new(&src[..32]);
        let status = if src[32] == 0 {
            PoolStatus::Uninitialized
        } else {
            match src[32] >> 6 {
                0 => PoolStatus::Unlocked,
                1 => PoolStatus::PendingOrder(
                    NonZeroU8::new((src[32] & STATUS_PENDING_ORDER_MASK) + 1)
                        .ok_or(ProgramError::InvalidArgument)?,
                ),
                2 => PoolStatus::Locked,
                3 => PoolStatus::LockedPendingOrder(
                    NonZeroU8::new((src[32] & STATUS_PENDING_ORDER_MASK) + 1)
                        .ok_or(ProgramError::InvalidArgument)?,
                ),
                _ => return Err(ProgramError::InvalidAccountData),
            }
        };
        Ok(Self {
            signal_provider,
            status,
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
        if let PoolStatus::Uninitialized = self.status {
            return false;
        }
        return true;
    }
}

impl Sealed for PoolAsset {}

impl IsInitialized for PoolAsset {
    fn is_initialized(&self) -> bool {
        self.mint_address != Pubkey::new(&[0u8; 32])
    }
}

impl Pack for PoolAsset {
    const LEN: usize = 40;

    fn pack_into_slice(&self, target: &mut [u8]) {
        let mint_address_bytes = self.mint_address.to_bytes();
        let amount_bytes = self.amount_in_token.to_le_bytes();
        for i in 0..32 {
            target[i] = mint_address_bytes[i]
        }

        for i in 32..40 {
            target[i] = amount_bytes[i - 32]
        }
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let mint_address = Pubkey::new(&src[..32]);
        let amount_in_token = u64::from_le_bytes(src[32..40].try_into().unwrap());
        Ok(Self {
            mint_address,
            amount_in_token,
        })
    }
}

pub fn unpack_assets(input: &[u8]) -> Result<Vec<PoolAsset>, ProgramError> {
    let number_of_assets = input.len() / PoolAsset::LEN;
    let mut output: Vec<PoolAsset> = Vec::with_capacity(number_of_assets);
    let mut offset = 0;
    for _ in 0..number_of_assets {
        PoolAsset::unpack(&input[offset..offset + PoolAsset::LEN])
            .and_then(|asset| Ok(output.push(asset)))
            .unwrap_or(());
        offset += PoolAsset::LEN;
    }
    Ok(output)
}

pub fn unpack_unchecked_asset(input: &[u8], index: usize) -> Result<PoolAsset, ProgramError> {
    let offset = PoolHeader::LEN + index * PoolAsset::LEN;
    input
        .get(offset..offset + PoolAsset::LEN)
        .ok_or(ProgramError::InvalidArgument)
        .and_then(|slice| PoolAsset::unpack_unchecked(slice))
}

pub fn pack_asset(target: &mut [u8], asset: &PoolAsset, index: usize) -> Result<(), ProgramError> {
    let offset = PoolHeader::LEN + index * PoolAsset::LEN;
    let slice = target
        .get_mut(offset..offset + PoolAsset::LEN)
        .ok_or(ProgramError::InvalidArgument)?;
    asset.pack_into_slice(slice);
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct OrderTracker {
    pub side: Side,
    pub source_amount_per_token: u64,
    pub pending_target_amount: u64,
    pub source_total_amount: u64
}

impl Sealed for OrderTracker {}

impl IsInitialized for OrderTracker {
    fn is_initialized(&self) -> bool {
        self.source_amount_per_token != 0
    }
}

impl Pack for OrderTracker {
    const LEN: usize = 32;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        dst[0] = match self.side {
            Side::Bid => 0,
            Side::Ask => 1,
        };

        let source_amount_per_token_bytes = self.source_amount_per_token.to_le_bytes();
        let pending_target_amount_bytes = self.pending_target_amount.to_le_bytes();
        let source_total_amount_bytes = self.source_total_amount.to_le_bytes();

        for i in 1..9 {
            dst[i] = source_amount_per_token_bytes[i - 1]
        }
        for i in 9..17 {
            dst[i] = pending_target_amount_bytes[i - 9]
        }
        for i in 17..25 {
            dst[i] = source_total_amount_bytes[i - 17]
        }
    }

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let side = match src[0] {
            0 => Side::Bid,
            1 => Side::Ask,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let source_amount_per_token = src
            .get(1..9)
            .map(|slice| u64::from_le_bytes(slice.try_into().unwrap()))
            .ok_or(ProgramError::InvalidAccountData)?;

        let pending_target_amount = src
            .get(9..17)
            .map(|slice| u64::from_le_bytes(slice.try_into().unwrap()))
            .ok_or(ProgramError::InvalidAccountData)?;

        let source_total_amount = src
            .get(17..25)
            .map(|slice| u64::from_le_bytes(slice.try_into().unwrap()))
            .ok_or(ProgramError::InvalidAccountData)?;

        Ok(Self {
            side,
            source_amount_per_token,
            pending_target_amount,
            source_total_amount
        })
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use super::{unpack_assets, OrderTracker, PoolAsset, PoolHeader, PoolStatus};
    use serum_dex::matching::Side;
    use solana_program::{
        program_pack::{IsInitialized, Pack},
        pubkey::Pubkey,
    };

    #[test]
    fn test_state_packing() {
        let header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            status: PoolStatus::PendingOrder(NonZeroU8::new(39).unwrap()),
        };

        let header_size = PoolHeader::LEN;
        let mut state_array = [0u8; 113];
        header_state.pack_into_slice(&mut state_array[..header_size]);

        let pool_asset = PoolAsset {
            mint_address: Pubkey::new_unique(),
            amount_in_token: 99,
        };
        let pool_asset_2 = PoolAsset {
            mint_address: Pubkey::new_unique(),
            amount_in_token: 499,
        };
        pool_asset.pack_into_slice(&mut state_array[header_size..]);
        pool_asset_2.pack_into_slice(&mut state_array[header_size + PoolAsset::LEN..]);

        let unpacked_header = PoolHeader::unpack(&state_array[..PoolHeader::LEN]).unwrap();
        assert_eq!(unpacked_header, header_state);

        let unpacked_pool_assets = unpack_assets(&state_array[PoolHeader::LEN..]).unwrap();
        assert_eq!(unpacked_pool_assets[0], pool_asset);
        assert_eq!(unpacked_pool_assets[1], pool_asset_2);
    }

    #[test]
    fn test_header_packing() {
        let mut header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            status: PoolStatus::PendingOrder(NonZeroU8::new(39).unwrap()),
        };
        assert_eq!(
            header_state,
            PoolHeader::unpack(&get_packed(&header_state)).unwrap()
        );

        header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            status: PoolStatus::LockedPendingOrder(NonZeroU8::new(64).unwrap()),
        };
        assert_eq!(
            header_state,
            PoolHeader::unpack(&get_packed(&header_state)).unwrap()
        );

        header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            status: PoolStatus::Locked,
        };
        assert_eq!(
            header_state,
            PoolHeader::unpack(&get_packed(&header_state)).unwrap()
        );

        header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            status: PoolStatus::Unlocked,
        };
        assert_eq!(
            header_state,
            PoolHeader::unpack(&get_packed(&header_state)).unwrap()
        );

        header_state = PoolHeader {
            signal_provider: Pubkey::new_unique(),
            status: PoolStatus::Uninitialized,
        };
        assert!(PoolHeader::unpack(&get_packed(&header_state)).is_err());
    }

    #[test]
    fn test_order_tracker_packing() {
        let mut order_tracker = OrderTracker {
            side: Side::Ask,
            source_amount_per_token: 54,
            pending_target_amount: 456,
            source_total_amount: 3212,
        };
        assert_eq!(
            order_tracker,
            OrderTracker::unpack(&get_packed(&order_tracker)).unwrap()
        );

        order_tracker.side = Side::Bid;
        order_tracker.source_amount_per_token = u64::MAX - 42;
        order_tracker.pending_target_amount = u64::MAX - 645;
        assert_eq!(
            order_tracker,
            OrderTracker::unpack(&get_packed(&order_tracker)).unwrap()
        );
    }

    fn get_packed<T: Pack>(obj: &T) -> Vec<u8> {
        let mut output_vec = vec![0u8].repeat(T::LEN);
        obj.pack_into_slice(&mut output_vec);
        output_vec
    }

    #[test]
    fn test_state_init() {
        let pool_asset = PoolAsset::unpack_unchecked(&[0u8; PoolAsset::LEN]).unwrap();
        assert!(!pool_asset.is_initialized());
    }
}
