use std::{convert::TryInto, num::NonZeroU8, str::FromStr};

#[cfg(feature = "fuzz")]
use arbitrary::Unstructured;

#[cfg(not(feature = "fuzz"))]
use bonfida_bot::{
    state::{unpack_assets, PoolHeader},
};

#[cfg(not(feature = "fuzz"))]
use bonfida_bot::state::FIDA_MINT_KEY;

#[cfg(feature = "fuzz")]
use crate::state::FIDA_MINT_KEY;
#[cfg(feature = "fuzz")]
use crate::state::{unpack_assets, PoolHeader};

use solana_program::{hash::Hash, instruction::{Instruction, InstructionError}, program_error::ProgramError, program_option::COption, program_pack::Pack, pubkey::Pubkey, rent::Rent, system_instruction};
use solana_program_test::{BanksClient, ProgramTest, ProgramTestBanksClientExt, ProgramTestContext, find_file, read_file};
use solana_sdk::{account::Account, signature::{Keypair, Signer}, transaction::{Transaction, TransactionError}, transport::TransportError};
use spl_associated_token_account::{create_associated_token_account, get_associated_token_address};
use spl_token::{instruction::initialize_account, state::Mint};

const SRM_MINT_KEY: &str = "SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt";

pub struct Context {
    pub bonfidabot_program_id: Pubkey,
    pub serum_program_id: Pubkey,
    pub test_state: ProgramTestContext,
    pub mint_authority: Keypair,
    pub fida_mint: MintInfo,
    pub srm_mint: MintInfo,
    pub pc_mint: MintInfo,
    pub coin_mint: MintInfo,
}

pub type MintInfo = (Pubkey, Mint);

impl Context {
    pub async fn refresh_blockhash(&mut self){
        self.test_state.last_blockhash = self.test_state.banks_client.get_new_blockhash(&self.test_state.last_blockhash).await.unwrap().0;
    }

    pub async fn init() -> Context {
        let bonfidabot_program_id = Pubkey::new_unique();
        let serum_program_id = Pubkey::new_unique();

        let mut program_test = ProgramTest::new(
            "bonfida_bot",
            bonfidabot_program_id,
            None
        );


        // Load The Serum Dex program
        program_test.add_account(
            serum_program_id,
            Account {
                lamports: u32::MAX.into(),
                data: read_file(find_file("serum_dex.so").unwrap()),
                owner: solana_program::bpf_loader_deprecated::id(),
                executable: true,
                ..Account::default()
            },
        );
        let payer = Keypair::new();

        program_test.add_account(
            payer.pubkey(),
            Account {
                lamports: 1<<63,
                ..Account::default()
            }
        );

        let mint_authority = Keypair::new();
        let fida_mint = mint_bootstrap(Some(FIDA_MINT_KEY), 6, &mut program_test, &mint_authority.pubkey());
        let srm_mint = mint_bootstrap(Some(SRM_MINT_KEY), 6, &mut program_test, &mint_authority.pubkey());
        let pc_mint = mint_bootstrap(None, 6, &mut program_test, &mint_authority.pubkey());
        let coin_mint = mint_bootstrap(None, 6, &mut program_test, &mint_authority.pubkey());

        let mut test_state = program_test.start_with_context().await;
        test_state.payer = payer;

        Context {
            bonfidabot_program_id,
            serum_program_id,
            test_state,
            mint_authority,
            fida_mint,
            srm_mint,
            pc_mint,
            coin_mint,
        }
    }

    pub fn get_mints(&self) -> Vec<MintInfo> {
        vec![
            self.fida_mint,
            self.srm_mint,
            self.pc_mint,
            self.coin_mint
        ]
    }
}


pub fn create_token_account(
    ctx: &Context,
    mint: &Pubkey,
    token_account: &Keypair,
    token_account_owner: &Pubkey,
) -> Transaction {
    let instructions = [
        system_instruction::create_account(
            &ctx.test_state.payer.pubkey(),
            &token_account.pubkey(),
            Rent::default().minimum_balance(165),
            165,
            &spl_token::id(),
        ),
        initialize_account(
            &spl_token::id(),
            &token_account.pubkey(),
            &mint,
            token_account_owner,
        )
        .unwrap(),
    ];
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&ctx.test_state.payer.pubkey()));
    transaction.partial_sign(&[&ctx.test_state.payer, token_account], ctx.test_state.last_blockhash);
    transaction
}

#[derive(Debug)]
pub struct OpenOrderView {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub native_coin_free: u64,
    pub native_coin_total: u64,
    pub native_pc_free: u64,
    pub native_pc_total: u64,
    pub orders: Vec<u128>,
}

impl OpenOrderView {
    pub fn parse(data: Vec<u8>) -> Self {
        let stripped = &data[13..];
        let market = Pubkey::new(&stripped[..32]);
        let owner = Pubkey::new(&stripped[32..64]);
        let native_coin_free = u64::from_le_bytes(stripped[64..72].try_into().unwrap());
        let native_coin_total = u64::from_le_bytes(stripped[72..80].try_into().unwrap());
        let native_pc_free = u64::from_le_bytes(stripped[80..88].try_into().unwrap());
        let native_pc_total = u64::from_le_bytes(stripped[88..96].try_into().unwrap());
        let mut orders = Vec::with_capacity(128);
        for i in 0..128 {
            orders.push(u128::from_le_bytes(
                stripped[(128 + 16 * i)..(144 + 16 * i)].try_into().unwrap(),
            ));
        }
        Self {
            market,
            owner,
            native_coin_free,
            native_coin_total,
            native_pc_free,
            native_pc_total,
            orders,
        }
    }

    pub async fn get(key: Pubkey, banks_client: &BanksClient) -> Self {
        Self::parse(
            banks_client
                .to_owned()
                .get_account(key)
                .await
                .unwrap()
                .unwrap()
                .data,
        )
    }
}

pub async fn print_pool_data(
    pool_key: &Pubkey,
    banks_client: &BanksClient,
) -> Result<(), ProgramError> {
    let data = banks_client
        .to_owned()
        .get_account(*pool_key)
        .await
        .unwrap()
        .unwrap()
        .data;
    let pool_header = PoolHeader::unpack(&data[..PoolHeader::LEN]).unwrap();
    let pool_asset_offset = PoolHeader::LEN + 32 * (pool_header.number_of_markets as usize);
    let pool_assets = unpack_assets(&data[pool_asset_offset..])?;
    for asset in pool_assets {
        print!("{:?}", asset);
        let pool_asset_key = get_associated_token_address(&pool_key, &asset.mint_address);
        let asset_data = banks_client
            .to_owned()
            .get_account(pool_asset_key)
            .await
            .unwrap()
            .unwrap()
            .data;
        let token_amount = spl_token::state::Account::unpack(&asset_data)?.amount;
        println!(" Token amount: {:?}", token_amount);
    }

    Ok(())
}

pub fn create_and_get_associated_token_address(
    payer_key: &Pubkey,
    parent_key: &Pubkey,
    mint_key: &Pubkey,
) -> (Instruction, Pubkey) {
    let create_source_asset_instruction =
        create_associated_token_account(payer_key, parent_key, mint_key);
    let source_asset_key = get_associated_token_address(parent_key, mint_key);
    return (create_source_asset_instruction, source_asset_key);
}

pub async fn wrap_process_transaction(
    ctx: &Context,
    instructions: Vec<Instruction>,
    mut signers: Vec<&Keypair>,
) -> Result<(), TransportError> {
    let mut setup_transaction = Transaction::new_with_payer(&instructions, Some(&ctx.test_state.payer.pubkey()));
    &signers.push(&ctx.test_state.payer);
    setup_transaction.partial_sign(&signers, ctx.test_state.last_blockhash);
    ctx.test_state.banks_client
        .to_owned()
        .process_transaction(setup_transaction)
        .await
}

pub fn add_token_account(
    program_test: &mut ProgramTest,
    account_address: Pubkey,
    owner_address: Pubkey,
    mint_address: Pubkey,
    amount: u64,
) {
    let mut token_data = [0; spl_token::state::Account::LEN];
    spl_token::state::Account {
        mint: mint_address,
        owner: owner_address,
        amount,
        delegate: COption::None,
        state: spl_token::state::AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    }
    .pack_into_slice(&mut token_data);
    program_test.add_account(
        account_address,
        Account {
            lamports: u32::MAX.into(),
            data: token_data.into(),
            owner: spl_token::id(),
            executable: false,
            ..Account::default()
        },
    );
}

pub fn mint_bootstrap(
    address: Option<&str>,
    decimals: u8,
    program_test: &mut ProgramTest,
    mint_authority: &Pubkey,
) -> MintInfo {
    let address = address
        .map(|s| Pubkey::from_str(s).unwrap())
        .unwrap_or_else(|| Pubkey::new_unique());
    let mint_info = Mint {
        mint_authority: Some(*mint_authority).into(),
        supply: u32::MAX.into(),
        decimals,
        is_initialized: true,
        freeze_authority: None.into(),
    };
    let mut data = [0; Mint::LEN];
    mint_info.pack_into_slice(&mut data);
    program_test.add_account(
        address,
        Account {
            lamports: u32::MAX.into(),
            data: data.into(),
            owner: spl_token::id(),
            executable: false,
            ..Account::default()
        },
    );
    (address, mint_info)
}

pub fn clone_keypair(k: &Keypair) -> Keypair {
    Keypair::from_bytes(&k.to_bytes()).unwrap()
}

#[cfg(feature = "fuzz")]
pub fn arbitraryNonZeroU8(u: &mut Unstructured<'_>) -> arbitrary::Result<NonZeroU8>{
    Ok(NonZeroU8::new(u.arbitrary()?).unwrap_or(NonZeroU8::new(1).unwrap()))
}

pub fn result_err_filter(e: Result<(), TransportError>) -> Result<(), TransportError>{
    if let Err(TransportError::TransactionError(te)) = &e {
        match te {
            TransactionError::InstructionError(_, ie) => {
                match ie {
                    InstructionError::InvalidArgument
                    | InstructionError::InvalidInstructionData
                    | InstructionError::InvalidAccountData
                    | InstructionError::InsufficientFunds
                    | InstructionError::AccountAlreadyInitialized
                    | InstructionError::InvalidSeeds
                    | InstructionError::Custom(2)
                    | InstructionError::Custom(3)
                    | InstructionError::Custom(4) => {Ok(())},
                    _ => {
                        print!("{:?}", ie);
                        e
                    }
                }
            },
            TransactionError::SignatureFailure
            | TransactionError::InvalidAccountForFee
            | TransactionError::InsufficientFundsForFee => {Ok(())},
            _ => {
                print!("{:?}", te);
                e
            }
        }
    } else {
        e
    }
}

pub fn get_element_from_seed<T>(choices: &Vec<T>, seed: u8) -> &T{
    &choices[(seed & choices.len() as u8) as usize]
}

// pub fn into_transport_error(e: ProgramError) -> TransportError {
//     TransportError::TransactionError(TransactionError::InstructionError(0,
//         match e {
//             ProgramError::Custom(u) => {InstructionError::Custom(u)}
//             ProgramError::InvalidArgument => {InstructionError::InvalidArgument}
//             ProgramError::InvalidInstructionData => {InstructionError::InvalidInstructionData}
//             ProgramError::InvalidAccountData => {InstructionError::InvalidAccountData}
//             ProgramError::AccountDataTooSmall => {InstructionError::AccountDataTooSmall}
//             ProgramError::InsufficientFunds => {InstructionError::InsufficientFunds}
//             ProgramError::IncorrectProgramId => {InstructionError::IncorrectProgramId}
//             ProgramError::MissingRequiredSignature => {InstructionError::MissingRequiredSignature}
//             ProgramError::AccountAlreadyInitialized => {InstructionError::AccountAlreadyInitialized}
//             ProgramError::UninitializedAccount => {InstructionError::UninitializedAccount}
//             ProgramError::NotEnoughAccountKeys => {InstructionError::NotEnoughAccountKeys}
//             ProgramError::AccountBorrowFailed => {InstructionError::AccountBorrowFailed}
//             ProgramError::MaxSeedLengthExceeded => {InstructionError::MaxSeedLengthExceeded}
//             ProgramError::InvalidSeeds => {InstructionError::InvalidSeeds}
//         }
//     ))
// }

pub fn into_transport_error(e: InstructionError) -> TransportError {
    TransportError::TransactionError(TransactionError::InstructionError(0, e))
}
