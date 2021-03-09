use bonfida_bot::{common::utils::{Context, mint_bootstrap}};
use futures::executor::block_on;
use honggfuzz::fuzz;
use solana_program::{pubkey::Pubkey};
use solana_program_test::{ProgramTest, find_file, read_file};
use solana_sdk::{account::Account, signature::{Keypair, Signer}};

use bonfida_bot::common::simulation::Execution;
use arbitrary::Arbitrary;

const SRM_MINT_KEY: &str = "SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt";


fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();

    loop {
        let mut ctx = rt.block_on(Context::init());
        let mints = ctx.get_mints();

        fuzz!(|e: Execution| {
            rt.block_on(e.run(&mut ctx, &mints));
        });
    }

}