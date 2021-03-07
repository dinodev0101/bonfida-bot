[![npm (scoped)](https://img.shields.io/npm/v/bonfida-bot)](https://www.npmjs.com/package/bonfida-bot)

# Bonfida-bot JS library

A JavaScript client library for interacting with the bonfida-bot on-chain program. This library can be used for :

- Creating pools.
- Trading on behalf of pools by sending signals.
- Depositing assets into a pool in exchange for pool tokens.
- Redeeming pool tokens to retrieve an investement from the pool.

## Installation

This library provides bindings for different pool operations. Adding it to a project is quite easy.
Using npm:

```
npm install bonfida-bot
```

Using yarn:

```
yarn add bonfida-bot
```

## Concepts

### Pool state and locking

There are two scenarios in which a pool can find itself in a _locked_ state :

- There are pending orders : some Serun orders have not yet been settled with the `settleFunds` instruction.
- There are fees to collect. At intervals greater than a week, signal provider and Bonfida fees must be extracted from the pool.

However, a _locked_ pool can be unlocked with a sequence of permissionless operations. When the pool has pending orders, it is often possible to
resolve the situation by running a `settleFunds` instruction. This is due to the fact that orders are either in the event queue and waiting to
be consumed by Serum permissionless crankers, or waiting to be settled. When the pool has fees to collect, the permissionless `collectFees` instruction extracts all accrued fees from the pool and transfers those to defined signal provider (50% of fee) and Bonfida wallets (25% of fee), as well as a FIDA buy and burn address (25% of fee).

TLDR: It is always possible for users to unlock the pool when they want to exit or even just enter into it.

## Usage

### Creating a pool

```ts
import { Connection, Account } from '@solana/web3.js';
import { createPool, } from 'bonfida-bot/instructions';
import { signAndSendTransactionInstructions, Numberu64, Numberu16 } from 'bonfida-bot/utils';
import { BONFIDABOT_PROGRAM_ID, SERUM_PROGRAM_ID, ENDPOINTS } from 'bonfida-bot/main';



const connection = new Connection(ENDPOINTS.mainnet);

const sourceOwnerAccount = Account([<private_key_array>]);
const signalProviderAccount = Account([<private_key_array>]);

// The payer account will pay for the fees involved with the transaction.
// For the sake of simplicity, we choose the source owner.
const payerAccount = sourceOwnerAccount

// Creating a pool means making the initial deposit.
// Any number of assets can be added. For each asset, the public key of the payer token account should be provided.
// The deposit amounts are also given
const sourceAssetKey = [
    new PublicKey("<First asset Key>"),
    new PublicKey("<Second asset Key>"),
]
const deposit_amounts = [1, 1];

// Maximum number of assets that can be held by the pool at any given time.
// A higher number means more flexibility but increases the initial pool account allocation fee.
const maxNumberOfAsset = 10;

// It is necessary for the purpose of security to hardcode all Serum markets that will be usable by the pool.
// It is advised to put here as many trusted markets with enough liquidity as required.
const allowedMarkets = [marketInfo.address];

// Interval of time in seconds between two fee collections. This must be greater or equal to a week.
// @ts-ignore
const feeCollectionPeriod = new Numberu64(604800);

// Total ratio of pool to be collected as fees at an interval defined by feeCollectionPeriod
// @ts-ignore
const feeRatio = 0.001;


// Create pool
let [poolSeed, createInstructions] = await createPool(
    connection,
    sourceOwnerAccount.publicKey
    sourceAssetKeys,
    signalProviderAccount.publicKey,
    deposit_amounts,
    maxNumberOfAsset,
    allowedMarkets,
    payerAccount.publicKey,
    // @ts-ignore
    feeCollectionPeriod,
    // @ts-ignore
    feeRatio
)

await signAndSendTransactionInstructions(
    connection,
    [sourceOwnerAccount], // The owner of the source asset accounts must sign this transaction.
    payerAccount,
    createInstructions
);
console.log("Created Pool")

```

### Signal provider operations

#### Sending an order to the pool as a signal provider

```ts
import { Connection, Account } from '@solana/web3.js';
import { createOrder } from 'bonfida-bot/instructions';
import { signAndSendTransactionInstructions, Numberu64, Numberu16 } from 'bonfida-bot/utils';

const connection = new Connection("https://mainnet-beta.solana.com");

let marketInfo = MARKETS[MARKETS.map(m => {return m.name}).lastIndexOf("<Market name, FIDA/USDC for instance>")];


// Each bonfida-bot pool is identified by a 32 byte pool seed
// This seed can be encoded as a base58 string which is similar to a public key.
let poolSeed = bs58.decode("<pool_seeds>");
let poolInfo = await fetchPoolInfo(connection, BONFIDABOT_PROGRAM_ID, poolSeed);

const signalProviderAccount = Account(<private_key_array>);

const payerAccount = signalProviderAccount;

let side = OrderSide.Ask;

// The limit price is defined as the number of price currency lots required to pay for a lot of coin currency.
// The coin lot size and price currency lot sizes are defined by the Serum market object.
// TODO: rework bindings to take in an actual price instead of bothering with lot sizes.
// @ts-ignore
let limit_price = new Numberu64(1<<63);
// @ts-ignore

// The max quanity is defined as the maximum amount of coin to be exchanged in the transaction.
// If the order is an Ask, this equates to the maximum quantity willing to be sold.
// If the order is a Bid, this equates to the maximum quantity which is required to be bought.
// TODO: In reality, it might be interesting to give more control by exposing the max price currency quantity to be exchanged in the case of a Bid order
// since matches can sometimes be more advantageous than the bid price.
let max_quantity = new Numberu16(1<<15);

// Until partial cancels of orders are implemented in Serum, the only supported order type for pools is IOC.
// This prevents long running limit orders from locking the pool and allows users to buy in and out of the pool
// at their owen convenience.
let order_type = OrderType.ImmediateOrCancel;

// The client_id can always be set to 0 for now. Its only use is to give the ability to refer to a particular order easily by this id in
// order to cancel it. Since only IOC orders are supported for now, the client_id can be set to any value.
// @ts-ignore
let client_id = new Numberu64(0);

// This only really matters for market makers. Since only IOC orders are supported for now, this parameter doesn't really matter.
let self_trade_behavior = SelfTradeBehavior.DecrementTake;

let [openOrderAccount, createPoolTxInstructions] = await createOrder(
    connection,
    BONFIDABOT_PROGRAM_ID,
    SERUM_PROGRAM_ID,
    poolInfo.seed,
    marketInfo.address,
    OrderSide.Ask,
    limit_price,
    max_quantity,
    order_type,
    client_id,
    self_trade_behavior,
    null, // Self referring
    payerAccount.publicKey
);

await signAndSendTransactionInstructions(
    connection,
    [openOrderAccount, signalProviderAccount], // Required transaction signers
    payerAccount,
    createPoolTxInstructions
);
console.log("Created Order for Pool")

```

### Non-privileged operations

#### Depositing funds into a pool

In exchange for a distribution of tokens which is proportional to the current asset holdings of the pool, a pool will issue pool tokens which
are redeemable for a commensurate proportion of pool assets at a later date. This operation represents the fundamental investment mechanism.

```ts
import { Connection, Account } from '@solana/web3.js';
import { deposit } from 'bonfida-bot/instructions';
import {
  signAndSendTransactionInstructions,
  Numberu64,
} from 'bonfida-bot/utils';
import { BONFIDABOT_PROGRAM_ID } from 'bonfida-bot/main';

// This value represents the maximum amount of pool tokens being requested by the user
// If the source asset accounts happen to be underfunded for this value to be reached,
// The pool will attempt to issue as many pool tokens as possible to the client while
// proportionately extracting funds from the source asset accounts.
// @ts-ignore
const poolTokenAmount = new Numberu64(3000000);

// Deposit into Pool
let depositTxInstructions = await deposit(
  connection,
  BONFIDABOT_PROGRAM_ID,
  sourceOwnerAccount.publicKey,
  sourceAssetKeys,
  poolTokenAmount,
  [poolInfo.seed],
  payerAccount.publicKey,
);

await signAndSendTransactionInstructions(
  connection,
  [sourceOwnerAccount], // Required transaction signer
  payerAccount,
  depositTxInstructions,
);
console.log('Deposited into Pool');
```

#### Retrieving funds from a pool

```ts
import { Connection, Account } from '@solana/web3.js';
import { redeem } from 'bonfida-bot/instructions';
import {
  signAndSendTransactionInstructions,
  Numberu64,
} from 'bonfida-bot/utils';
import { BONFIDABOT_PROGRAM_ID } from 'bonfida-bot/main';

// By setting this value to be lower than the actual pool token balance of the pool token account,
// It is possible to partially redeem assets from a pool.
// @ts-ignore
const poolTokenAmount = new Numberu64(1000000);

let redeemTxInstruction = await redeem(
  connection,
  BONFIDABOT_PROGRAM_ID,
  sourceOwnerAccount.publicKey,
  sourcePoolTokenKey,
  sourceAssetKeys,
  [poolInfo.seed],
  [poolTokenAmount],
);

await signAndSendTransactionInstructions(
  connection,
  [sourceOwnerAccount], // Required transaction signer
  payerAccount,
  redeemTxInstruction,
);
console.log('Redeemed out of Pool');
```

### Settling funds from an order

Once a Serum order has gone through, it is necessary to retrieve the funds from the openOrder account in order to unlock the pool for all deposit and
redeem operations. Thankfully, this operation is permissionless which means that a locked pool is unlockable by anyone. This means that in order to make sure that a deposit or redeem instruction will go through, it is interesting to precede it with a settle instruction in the same transaction.

```ts
import { Connection, Account } from '@solana/web3.js';
import { settleFunds } from 'bonfida-bot/instructions';
import { signAndSendTransactionInstructions } from 'bonfida-bot/utils';

const payerAccount = Account(<private_key_array>);

let settleFundsTxInstructions = await settleFunds(
  connection,
  BONFIDABOT_PROGRAM_ID,
  SERUM_PROGRAM_ID,
  poolInfo.seed,
  marketInfo.address,
  OpenOrderAccount.address,
  null,
  );

await signAndSendTransactionInstructions(
    connection,
    [], // No signer is required for this transaction! (Except to pay for transaction fees)
    payerAccount,
    settleFundsTxInstructions
  );
console.log("Settled Funds")

```

### Triggering a fee collection operation

In order to unlock a potentially unlocked pool, or in order to trigger fee collection as a signal provider, it is necesary to
activate the `collectFees` permissionless crank.

| Beneficiary       | Fee Proportion |
| ----------------- | -------------- |
| Signal provider   | 50%            |
| Bonfida           | 25%            |
| FIDA buy and burn | 25%            |

```ts
import { Connection, Account } from '@solana/web3.js';
import { collectFees } from 'bonfida-bot/instructions';
import { signAndSendTransactionInstructions } from 'bonfida-bot/utils';

let collectFeesTxInstruction = await collectFees(
  connection,
  BONFIDABOT_PROGRAM_ID,
  [poolInfo.seed],
);

await signAndSendTransactionInstructions(
  connection,
  [],
  payerAccount,
  collectFeesTxInstruction,
);
console.log('Redeemed out of Pool');
```
