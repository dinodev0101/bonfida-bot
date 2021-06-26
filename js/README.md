[![npm (scoped)](https://img.shields.io/npm/v/@bonfida/bot)](https://www.npmjs.com/package/@bonfida/bot)

# @bonfida/bot JS library

A JavaScript client library for interacting with the @bonfida/bot on-chain program. This library can be used for :

- Creating pools.
- Trading on behalf of pools by sending signals.
- Depositing assets into a pool in exchange for pool tokens.
- Redeeming pool tokens to retrieve an investement from the pool.

## Installation

This library provides bindings for different pool operations. Adding it to a project is quite easy.
Using npm:

```
npm install @bonfida/bot
```

Using yarn:

```
yarn add @bonfida/bot
```

## Concepts

### Pool state and locking

There are two scenarios in which a pool can find itself in a _locked_ state :

- There are pending orders : some Serum orders have not yet been settled with the `settleFunds` instruction.
- There are fees to collect. At intervals greater than a week, signal provider and Bonfida fees must be extracted from the pool.

However, a _locked_ pool can be unlocked with a sequence of permissionless operations. When the pool has pending orders, it is often possible to
resolve the situation by running a `settleFunds` instruction. This is due to the fact that orders are either in the event queue and waiting to
be consumed by Serum permissionless crankers, or waiting to be settled. When the pool has fees to collect, the permissionless `collectFees` instruction extracts all accrued fees from the pool and transfers those to defined signal provider (50% of fee) and Bonfida Bots Insurance Fund (25% of fee), as well as a FIDA buy and burn address (25% of fee).

TLDR: It is always possible for users to unlock the pool when they want to exit or even just enter into it.

## Usage

### Creating a pool

```ts
import { Connection, Account, PublicKey } from '@solana/web3.js';
import { createPool } from '@bonfida/bot';
import { signAndSendTransactionInstructions, Numberu64 } from '@bonfida/bot';
import { ENDPOINTS } from '@bonfida/bot';

const connection = new Connection(ENDPOINTS.mainnet);

const sourceOwnerAccount = new Account(['<MY PRIVATE KEY ARRAY>']);
const signalProviderAccount = new Account(['<MY PRIVATE KEY ARRAY AGAIN>']);

// The payer account will pay for the fees involved with the transaction.
// For the sake of simplicity, we choose the source owner.
const payerAccount = sourceOwnerAccount;

// Creating a pool means making the initial deposit.
// Any number of assets can be added. For each asset, the public key of the payer token account should be provided.
// The deposit amounts are also given, with the precision (mostly 6 decimals) of the respective token.
const sourceAssetKeys = [
  new PublicKey('<First asset Key>'),
  new PublicKey('<Second asset Key>'),
];
const deposit_amounts = [1000000, 1000000]; // This corresponds to 1 token of each with 6 decimals precisions

//Fetching the number of decimals using the mint of a token can be done with:
export const decimalsFromMint = async (
  connection: Connection,
  mint: PublicKey,
) => {
  const result = await connection.getParsedAccountInfo(mint);
  // @ts-ignore
  const decimals = result?.value?.data?.parsed?.info?.decimals;
  if (!decimals) {
    throw new Error('Invalid mint');
  }
  return decimals;
};

// Maximum number of assets that can be held by the pool at any given time.
// A higher number means more flexibility but increases the initial pool account allocation fee.
const maxNumberOfAsset = 10;

// It is necessary for the purpose of security to hardcode all Serum markets that will be usable by the pool.
// It is advised to put here as many trusted markets with enough liquidity as required.
const allowedMarkets = [
  new PublicKey('E14BKBhDWD4EuTkWj1ooZezesGxMW8LPCps4W5PuzZJo'),
]; // FIDA/USDC market address, for example
// Market addresses can be fetched with:
let marketAddress =
  MARKETS[
    MARKETS.map(m => {
      return m.name;
    }).lastIndexOf('<Market name, FIDA/USDC for instance>')
  ].address;

// Interval of time in seconds between two fee collections. This must be greater or equal to a week (604800 s).
// @ts-ignore
const feeCollectionPeriod = new Numberu64(604800);

// Total percentage of the pool to be collected as fees (which are split up between teh signal provider and Bonfida) at an interval defined by feeCollectionPeriod
// @ts-ignore
const feePercentage = 0.1;

const pool = async () => {
  // Create pool
  let [poolSeed, createInstructions] = await createPool(
    connection,
    sourceOwnerAccount.publicKey,
    sourceAssetKeys,
    signalProviderAccount.publicKey,
    deposit_amounts,
    maxNumberOfAsset,
    allowedMarkets,
    payerAccount.publicKey,
    // @ts-ignore
    feeCollectionPeriod,
    // @ts-ignore
    feePercentage,
  );

  await signAndSendTransactionInstructions(
    connection,
    [sourceOwnerAccount], // The owner of the source asset accounts must sign this transaction.
    payerAccount,
    createInstructions,
  );
  console.log('Created Pool');
};

pool();
```

### Signal provider operations

#### Sending an order to the pool as a signal provider

```ts
import { MARKETS } from '@project-serum/serum';
import { Connection, Account } from '@solana/web3.js';
import {
  createOrder,
  ENDPOINTS,
  OrderSide,
  OrderType,
  SelfTradeBehavior,
} from '@bonfida/bot';
import { signAndSendTransactionInstructions, Numberu64 } from '@bonfida/bot';
import bs58 from 'bs58';

const connection = new Connection(ENDPOINTS.mainnet);

let marketInfo =
  MARKETS[
    MARKETS.map(m => {
      return m.name;
    }).lastIndexOf('<Market name, FIDA/USDC for instance>')
  ];

// Each @bonfida/bot pool is identified by a 32 byte pool seed
// This seed can be encoded as a base58 string which is similar to a public key.
let poolSeed = bs58.decode('<poolSeeds>');

const signalProviderAccount = new Account('<MY PRIVATE KEY ARRAY>');

const payerAccount = signalProviderAccount;

let side = OrderSide.Ask;

// The limit price is defined as the number of price currency lots required to pay for a lot of coin currency.
// The coin lot size and price currency lot sizes can be fetched via getMarketData from '@bonfida/bot'
// @ts-ignore
let limitPrice = new Numberu64(100000);

// The max quantity is defined as the maximum percentage of available coin assets to be exchanged in the transaction from the pool.
// If the order is an Ask, this equates to the maximum quantity willing to be sold.
// If the order is a Bid, this equates to the maximum quantity which is required to be bought.
// If the amountToTrade argument is specified to createOrder, it will overwrite this percentage with the absolute amount given.
let maxQuantityPercentage = 20;

// Until partial cancels of orders are implemented in Serum, the only supported order type for pools is IOC.
// This prevents long running limit orders from locking the pool and allows users to buy in and out of the pool
// at their owen convenience.
let orderType = OrderType.ImmediateOrCancel;

// The client_id can always be set to 0 for now. Its only use is to give the ability to refer to a particular order easily by this id in
// order to cancel it. Since only IOC orders are supported for now, the client_id can be set to any value.
// @ts-ignore
let clientId = new Numberu64(0);

// This only really matters for market makers. Since only IOC orders are supported for now, this parameter doesn't really matter.
let selfTradeBehavior = SelfTradeBehavior.DecrementTake;

const order = async () => {
  let [openOrderAccount, createPoolTxInstructions] = await createOrder(
    connection,
    poolSeed,
    marketInfo.address,
    side,
    limitPrice,
    maxQuantityPercentage,
    orderType,
    clientId,
    selfTradeBehavior,
    null, // Self referring
    payerAccount.publicKey,
  );

  await signAndSendTransactionInstructions(
    connection,
    [openOrderAccount, signalProviderAccount], // Required transaction signers
    payerAccount,
    createPoolTxInstructions,
  );
  console.log('Created Order for Pool');
};

order();
```

### Non-privileged operations

#### Depositing funds into a pool

In exchange for a distribution of tokens which is proportional to the current asset holdings of the pool, a pool will issue pool tokens which
are redeemable for a commensurate proportion of pool assets at a later date. This operation represents the fundamental investment mechanism.

```ts
import { deposit } from '@bonfida/bot';
import { signAndSendTransactionInstructions, Numberu64 } from '@bonfida/bot';
import { BONFIDABOT_PROGRAM_ID } from '@bonfida/bot';

// This value represents the maximum amount of pool tokens being requested by the user
// If the source asset accounts happen to be underfunded for this value to be reached,
// The pool will attempt to issue as many pool tokens as possible to the client while
// proportionately extracting funds from the source asset accounts.
// The number of decimals is 6.
// @ts-ignore
const poolTokenAmount = new Numberu64(3000000);

const depositIntoPool = async () => {
  // Deposit into Pool.
  let depositTxInstructions = await deposit(
    connection,
    sourceOwnerAccount.publicKey,
    sourceAssetKeys,
    poolTokenAmount,
    [poolSeed],
    payerAccount.publicKey,
  );

  await signAndSendTransactionInstructions(
    connection,
    [sourceOwnerAccount], // Required transaction signer
    payerAccount,
    depositTxInstructions,
  );
  console.log('Deposited into Pool');
};

depositIntoPool();
```

#### Retrieving funds from a pool

```ts
import { redeem } from '@bonfida/bot';
import { signAndSendTransactionInstructions, Numberu64 } from '@bonfida/bot';

// By setting this value to be lower than the actual pool token balance of the pool token account,
// It is possible to partially redeem assets from a pool.
// @ts-ignore
const poolTokenAmount = new Numberu64(1000000);

const redeemFromPool = async () => {
  let redeemTxInstruction = await redeem(
    connection,
    sourceOwnerAccount.publicKey,
    sourcePoolTokenKey,
    sourceAssetKeys,
    [poolSeed],
    poolTokenAmount,
  );

  await signAndSendTransactionInstructions(
    connection,
    [sourceOwnerAccount], // Required transaction signer
    payerAccount,
    redeemTxInstruction,
  );
  console.log('Redeemed out of Pool');
};

redeemFromPool();
```

### Settling funds from an order

Once a Serum order has gone through, it is necessary to retrieve the funds from the openOrder account in order to unlock the pool for all deposit and
redeem operations. Thankfully, this operation is permissionless which means that a locked pool is unlockable by anyone. This means that in order to make sure that a deposit or redeem instruction will go through, it is interesting to precede it with a settle instruction in the same transaction.

```ts
import { Account } from '@solana/web3.js';
import { settleFunds } from '@bonfida/bot';
import { signAndSendTransactionInstructions } from '@bonfida/bot';

const payerAccount = Account('<MY PRIVATE KEY ARRAY>');

// It is also possible to settle all openOrders for a pool via getPoolOrderInfos from '@bonfida/bot'
let settleFundsTxInstructions = await settleFunds(
  connection,
  poolSeed,
  marketAddress,
  OpenOrderAccount.address,
  null,
);

await signAndSendTransactionInstructions(
  connection,
  [], // No signer is required for this transaction! (Except to pay for transaction fees)
  payerAccount,
  settleFundsTxInstructions,
);
console.log('Settled Funds');
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
import { collectFees } from '@bonfida/bot';
import { signAndSendTransactionInstructions } from '@bonfida/bot';

let collectFeesTxInstruction = await collectFees(connection, [poolSeed]);

await signAndSendTransactionInstructions(
  connection,
  [],
  payerAccount,
  collectFeesTxInstruction,
);
console.log('Redeemed out of Pool');
```
