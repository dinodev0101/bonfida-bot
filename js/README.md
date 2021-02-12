# Simple JS binding

## Quickstart

Run `yarn` in the `js` directory to install the node modules. Run `yarn dev` to get started and `yarn build` to build.

Contract address on Devnet

```
Hj9R6bEfrULLNrApMsKCEaHR9QJ2JgRtM381xgYcjFmQ
```

See on the [Solana Explorer](https://explorer.solana.com/address/Hj9R6bEfrULLNrApMsKCEaHR9QJ2JgRtM381xgYcjFmQ?cluster=devnet)

The code allows you to

- Create vesting instructions for any SPL token: `createCreateInstruction`
- Create unlock instructions: `createUnlockInstruction`
- Change the destination of the vested tokens: `createChangeDestinationInstruction`

(To import Solana accounts created with [Sollet](https://sollet.io) you can use `getAccountFromSeed`)

```
Seed 9043936629442508205162695100279588102353854608998701852963634059
Vesting contract account pubkey:  r2p2mLJvyrTzetxxsttQ54CS1m18zMgYqKSRzxP9WpE
contract ID:  90439366294425082051626951002795881023538546089987018529636340fe
✅ Successfully created vesting instructions
🚚 Transaction signature: 2uypTM3QcroR7uk6g9Y4eLdniCHqdQBDq4XyrFM7hCtTbb4rftkEHMM6vJ6tTYpihYubHt55xWD86vHB857bqXXb

Fetching contract  r2p2mLJvyrTzetxxsttQ54CS1m18zMgYqKSRzxP9WpE
✅ Successfully created unlocking instructions
🚚 Transaction signature: 2Vg3W1w8WBdRAWBEwFTn2BtMkKPD3Xor7SRvzC193UnsUnhmneUChPHe7vLF9Lfw9BKxWH5JbbJmnda4XztHMVHz

Fetching contract  r2p2mLJvyrTzetxxsttQ54CS1m18zMgYqKSRzxP9WpE
✅ Successfully changed destination
🚚 Transaction signature: 4tgPgCdM5ubaSKNLKD1WrfAJPZgRajxRSnmcPkHcN1TCeCRmq3cUCYVdCzsYwr63JRf4D2K1UZnt8rwu2pkGxeYe
```

## Testing Keys for the devnet

KEYS: 

Program_id: EfUL1dkbEXE5UbjuZpR3ckoF4a8UCuhCVXbzTFmgQoqA

source_owner: ~/.config/solana/id.json
Pubkey: FbqE3zeiu8ccBgt1xA6F5Yx8bq5T1D5j9eUcqFs4Dsvb

token_mint: 3wmMWPDkSdKd697arrGWYJ1q4QL1jwGxnANUyXqSV9vC
source_token: EWrBFuSdmMC3wQKvWaCUTLbDhQT3Mpmw2CVViK4P5Xk2

token_mint: CwAkhuLpTdTBRQHxGeWQmHp8oqFRetLoN3rW2xpz23eH
source_token: 2esFPgbjM4b5LvX73LhwFdZ2gadbCjf6YSCULanDuboo


CMDS (don't forget the url):

```
solana-keygen new --outfile ~/.config/solana/id.json
solana-keygen new --outfile ~/.config/solana/id_dest.json
solana-keygen new --outfile ~/.config/solana/id_new_dest.json
solana airdrop 10 --url https://devnet.solana.com ~/.config/solana/id.json
solana deploy ../program/target/deploy/token_vesting.so --url https://devnet.solana.com

spl-token create-token
spl-token create-account 3wmMWPDkSdKd697arrGWYJ1q4QL1jwGxnANUyXqSV9vC --url https://devnet.solana.com --owner KEYPAIR
spl-token mint 3wmMWPDkSdKd697arrGWYJ1q4QL1jwGxnANUyXqSV9vC 10000 --url https://devnet.solana.com --owner ~/.config/solana/id.json

echo "RUST_BACKTRACE=1 ./target/debug/vesting-contract-cli                          \
--url https://devnet.solana.com                                                     \
--program_id 5eiTBnbpMsioMR7TbFPLxpj7KLi9c8esrZXYzuW9uEgy                           \
create                                                                              \
--mint_address 3wmMWPDkSdKd697arrGWYJ1q4QL1jwGxnANUyXqSV9vC                         \
--source_owner ~/.config/solana/id.json                                             \
--destination_address 8vBVs9hATt4C4DeMfheiqJ7kJhX9JQffDQ9bJW4dN7nX                  \
--amounts 2,1,3,!                                                                   \
--release-heights 1,28504431,28506000,!                                             \
--payer ~/.config/solana/id.json" | bash               


echo "RUST_BACKTRACE=1 ./target/debug/vesting-contract-cli                          \
--url https://devnet.solana.com                                                     \
--program_id 5eiTBnbpMsioMR7TbFPLxpj7KLi9c8esrZXYzuW9uEgy                           \
info                                                                                \
--seed LX3EUdRUBUa3TbsYXLEUdj9J3prXkWXvLYSWyYyc2P5 " | bash                                          


echo "RUST_BACKTRACE=1 ./target/debug/vesting-contract-cli                          \
--url https://devnet.solana.com                                                     \
--program_id 5eiTBnbpMsioMR7TbFPLxpj7KLi9c8esrZXYzuW9uEgy                           \
change-destination                                                                  \
--seed LX3EUdRUBUa3TbsYXLEUdj9J3prXkWXvLYSWyYyc2P5                                  \
--current_destination_owner ~/.config/solana/id_dest.json                           \
--new_destination_token_address CrCPEHiRz2bpC3kmtu3vdghhL62GFeRnUeck8RYNBQkh        \
--payer ~/.config/solana/id.json" | bash                           


echo "RUST_BACKTRACE=1 ./target/debug/vesting-contract-cli                          \
--url https://devnet.solana.com                                                     \
--program_id 5eiTBnbpMsioMR7TbFPLxpj7KLi9c8esrZXYzuW9uEgy                           \
unlock                                                                              \
--seed LX3EUdRUBUa3TbsYXLEUdj9J3prXkWXvLYSWyYyc2P5                                  \
--payer ~/.config/solana/id.json" | bash
```

LINKS:

https://spl.solana.com/token
