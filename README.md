# bonfida-bot

This code is unaudited. Use at your own risk.

Bonfida-bot is a an on-chain and off-chain tool suite that enables the creation of trading pools. Each trading pool has a cryptographically identified signal provider who is able to create Serum orders with the pool's assets. Anyone can invest in a trading pool in exchange for specific pool tokens which are redeemable at any time.
### Structure:

- Program account data:

  - Vec of pool tokens accounts addresses (Vec of Pubkeys, owned by program)
  - Pool FIDA account address (Pubkey, owned by program)
  - Signal Provider address (Pubkey)

- Instructions:
  - Initialize Pool:
    - Max number of Markets (u32)
    - Signed by Payer
  - Deposit(initializes the pool if its nonexistent):
    - Pool Seeds (256 bits)
    - Token Mint address (Pubkey)
    - Amount (u64)
    - Signed by Source and Payer
  - Redeem:
    - Pool Seeds (256 bits)
    - Amount of Pooltoken (u64)
    - Payout Token Mint address (Pubkey)
    - Payout Destination token address (Pubkey)
    - Signed by Pool-token owner and Payer
  - Trade (from signal):
    - Pool Seeds (256 bits)
    - Array of order amounts (array of u64)
    - Serum Market address (Pubkey?)
    - Buy/Sell (bool, buy = true)
    - Signed by Signal Provider and Payer

### Diagram:

![structure-diagram](assets/bonfida-bot.png)

### Build and use:

Run `make` in the `program` folder before doing any testing or fuzzing.

### Security considerations

The pools are designed with several security considerations in mind :

- The signal provider is never directly in control of the pool's asset. They can only issue Serum market orders on behalf of the pool.

- A signal provider is contractually obligated to perform market operations on a specific set of markets which is immutably defined at pool creation.
  This means that it is impossible for the signal provider to directly extract assets from the pool by creating temporary mock markets which would enable the signal provider from buying the pool's asset under the market price.

- Whereas the pool can itself be in a _locked_ state which locally prevents pool token redeeming as well as investments, it is always possible for anyone to unlock the pool in order to gain access to their funds or just buy in.

### See also

- [JS library repo link](js)