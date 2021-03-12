import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_CLOCK_PUBKEY,
  TransactionInstruction,
  Connection,
  CreateAccountParams,
  InstructionType,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, AccountLayout } from '@solana/spl-token';
import {
  cancelOrderInstruction,
  collectFeesInstruction,
  createInstruction,
  createOrderInstruction,
  depositInstruction,
  initInstruction,
  redeemInstruction,
  settleFundsInstruction,
} from './instructions';
import {
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  Numberu64,
  Numberu16,
  getMarketData,
  Numberu128,
  findAndCreateAssociatedAccount,
} from './utils';
import {
  OrderSide,
  OrderType,
  PoolAsset,
  PoolHeader,
  SelfTradeBehavior,
  unpack_assets,
  PUBKEY_LENGTH,
  unpack_markets,
} from './state';
import bs58 from 'bs58';
import * as crypto from 'crypto';
import { open } from 'fs/promises';
import { OpenOrders } from '@project-serum/serum';

/////////////////////////////////

export const ENDPOINTS = {
  mainnet: 'https://solana-api.projectserum.com',
  devnet: 'https://devnet.solana.com',
  mainnet2: 'https://api.mainnet-beta.solana.com'
};

export const BONFIDABOT_PROGRAM_ID: PublicKey = new PublicKey(
  '63xyXHpA6EVF69kRmEbXAr8aBEkhgpaNUSRoTQyi5Rwr',
);

export const BONFIDA_FEE_KEY: PublicKey = new PublicKey(
  '31LVSggbVz4VcwBSPdtK8HJ3Lt1cKTJUVQTRNNYMfqBq',
);

export const BONFIDA_BNB_KEY: PublicKey = new PublicKey(
  '3oQzjfjzUkJ5qHsERk2JPEpAKo34dxAQjUriBqursfxU',
);

export const SERUM_PROGRAM_ID: PublicKey = new PublicKey(
  '9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin',
);

export const FIDA_KEY: PublicKey = new PublicKey(
  'EchesyfXePKdLtoiZSL8pBe8Myagyy8ZRqsACNCFGnvp',
);

export const PUBLIC_POOLS_SEEDS = [
  new PublicKey('HFhJ3k84H3K3iCNHbkTZ657r9fwQGux7czUZavhm4ebV'),
];

/////////////////////////////////

/**
 * Returns the solana instructions to create a new pool by performing the first deposit of any number of different tokens
 * and setting the pubkey of the signal provider. The first deposit will fix the initial
 * value of 1 pooltoken (credited to the target) with respect to the deposited tokens.
 * (Signed by the sourceOwner account)
 *
 * @param connection The connection object to the rpc node
 * @param sourceOwnerKey The address of the wallet that owns the tokens to be invested in the pool
 * @param sourceAssetKeys The adresses of the token accounts that hold the tokens to be invested in the pool
 * @param signalProviderKey The key of the account that will have the right to trade with the funds of the pool
 * @param depositAmounts An array of the amounts that should be invested for each token, in the same order as the sourceAssetKeys
 * @param maxNumberOfAssets The maximum number of different tokens the pool will ever be able to hold (solana memory allocation is fixed)
 * @param markets An array of the addresses of the serum markets that the signalProvider will be able to trade on
 * @param payer The address of the account that should pay for the allocation fees
 * @param feeCollectionPeriod The smallest period in seconds after which the trading fees can be payed out again (minimum is 604800 s or 1 week)
 * @param feePercentage The percentage (a number from 0 to 100) of the pool assets that should be collected as fees
 */
export async function createPool(
  connection: Connection,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  signalProviderKey: PublicKey,
  depositAmounts: Array<number>,
  maxNumberOfAssets: number,
  markets: Array<PublicKey>,
  payer: PublicKey,
  feeCollectionPeriod: Numberu64,
  feePercentage: number,
): Promise<[Uint8Array, TransactionInstruction[]]> {

  // Find a valid pool seed
  let poolSeed: Uint8Array;
  let poolKey: PublicKey;
  let bump: number;
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  while (true) {
    poolSeed = crypto.randomBytes(32);
    [poolKey, bump] = await PublicKey.findProgramAddress(
      [poolSeed.slice(0, 31)],
      BONFIDABOT_PROGRAM_ID,
    );
    poolSeed[31] = bump;
    try {
      await PublicKey.createProgramAddress(
        [poolSeed, array_one],
        BONFIDABOT_PROGRAM_ID,
      );
      break;
    } catch (e) {
      continue;
    }
  }
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    BONFIDABOT_PROGRAM_ID,
  );
  console.log('Pool seed: ', bs58.encode(poolSeed));
  console.log('Pool key: ', poolKey.toString());
  console.log('Mint key: ', poolMintKey.toString());

  // Initialize the pool
  // @ts-ignore
  let numberOfMarkets = new Numberu16(markets.length)
  let initTxInstruction = initInstruction(
    TOKEN_PROGRAM_ID,
    SystemProgram.programId,
    SYSVAR_RENT_PUBKEY,
    BONFIDABOT_PROGRAM_ID,
    poolMintKey,
    payer,
    poolKey,
    [poolSeed],
    maxNumberOfAssets,
    numberOfMarkets,
  );

  // Create the pool asset accounts
  let poolAssetKeys: PublicKey[] = new Array();
  let assetTxInstructions: TransactionInstruction[] = new Array();
  for (var sourceAssetKey of sourceAssetKeys) {
    let assetInfo = await connection.getAccountInfo(sourceAssetKey);
    if (!assetInfo) {
      throw 'Source asset account is unavailable';
    }
    let assetData = Buffer.from(assetInfo.data);
    const assetMint = new PublicKey(AccountLayout.decode(assetData).mint);
    assetTxInstructions.push(
      await createAssociatedTokenAccount(
        SystemProgram.programId,
        payer,
        poolKey,
        assetMint,
      ),
    );
    poolAssetKeys.push(await findAssociatedTokenAddress(poolKey, assetMint));
  }

  // If nonexistent, create the source owner associated addresses to receive the pooltokens
  let txInstructions: Array<TransactionInstruction> = [initTxInstruction];
  let [targetPoolTokenKey, targetPTInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    sourceOwnerKey,
    poolMintKey,
    payer
  );
  targetPTInstruction? txInstructions.push(targetPTInstruction) : null;

  // Create the pool
  // @ts-ignore
  let feeRatioU16 = new Numberu16(2**16 * feePercentage / 100);
  let createTxInstruction = createInstruction(
    TOKEN_PROGRAM_ID,
    BONFIDABOT_PROGRAM_ID,
    SYSVAR_CLOCK_PUBKEY,
    poolMintKey,
    poolKey,
    [poolSeed],
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwnerKey,
    sourceAssetKeys,
    SERUM_PROGRAM_ID,
    signalProviderKey,
    depositAmounts,
    markets,
    feeCollectionPeriod,
    feeRatioU16,
  );
  txInstructions = txInstructions.concat(assetTxInstructions);
  txInstructions.push(createTxInstruction);

  return [poolSeed, txInstructions];
}


/**
 * Returns the solana instructions to buy into the pool. The source deposits tokens into the pool and the target receives
 * a corresponding amount of pool-token in exchange. The program will try to
 * maximize the deposit sum with regards to the amounts given by the source and
 * the ratio of tokens present in the pool at that moment. Tokens can only be deposited
 * in the exact ratio of tokens that are present in the pool.
 * (Signed by the sourceOwnerKey)
 *
 * @param connection The connection object to the rpc node
 * @param sourceOwnerKey The address of the wallet that owns the tokens to be invested in the pool
 * @param sourceAssetKeys The adresses of the token accounts that hold the tokens to be invested in the pool
 * @param poolTokenAmount The amount of pooltokens that should be bought (ie the amount of tokens that should be invested)
 * @param poolSeed The seed of the pool that should be invested into
 * @param payer The address of the account that should pay for the allocation fees
 */
export async function deposit(
  connection: Connection,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  poolTokenAmount: Numberu64,
  poolSeed: Array<Buffer | Uint8Array>,
  payer: PublicKey,
): Promise<TransactionInstruction[]> {

  // Find the pool key and mint key
  let poolKey = await PublicKey.createProgramAddress(
    poolSeed,
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    poolSeed.concat(array_one),
    BONFIDABOT_PROGRAM_ID,
  );

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolData = poolInfo.data;
  let poolHeader = PoolHeader.fromBuffer(poolData.slice(0, PoolHeader.LEN));
  let poolAssets: Array<PoolAsset> = unpack_assets(
    poolData.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let poolAssetKeys: Array<PublicKey> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    poolAssetKeys.push(assetKey);
  }

  // If nonexistent, create the source owner and signal provider associated addresses to receive the pooltokens
  let createTargetsTxInstructions: Array<TransactionInstruction> = [];
  let [targetPoolTokenKey, targetPTInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    sourceOwnerKey,
    poolMintKey,
    payer
  );
  targetPTInstruction? createTargetsTxInstructions.push(targetPTInstruction) : null;

  let [sigProviderFeeReceiverKey, sigProvInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    poolHeader.signalProvider,
    poolMintKey,
    payer
  );
  sigProvInstruction? createTargetsTxInstructions.push(sigProvInstruction) : null;

  let [bonfidaFeeReceiverKey, bonfidaFeeInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    BONFIDA_FEE_KEY,
    poolMintKey,
    payer
  );
  bonfidaFeeInstruction? createTargetsTxInstructions.push(bonfidaFeeInstruction) : null;

  let [bonfidaBuyAndBurnKey, bonfidaBNBInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    BONFIDA_BNB_KEY,
    poolMintKey,
    payer
  );
  bonfidaBNBInstruction? createTargetsTxInstructions.push(bonfidaBNBInstruction) : null;

  let depositTxInstruction = depositInstruction(
    TOKEN_PROGRAM_ID,
    BONFIDABOT_PROGRAM_ID,
    sigProviderFeeReceiverKey,
    bonfidaFeeReceiverKey,
    bonfidaBuyAndBurnKey,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwnerKey,
    sourceAssetKeys,
    poolSeed,
    poolTokenAmount,
  );
  return createTargetsTxInstructions.concat(depositTxInstruction);
}

/**
 * Returns the solana instructions to create a new serum order for the pool.
 * (Signed by the SignalProvider account of the pool and the OpenOrder Account
 * returned by this function)
 *
 * @param connection The connection object to the rpc node
 * @param poolSeed The seed of the pool that should be traded on
 * @param market The address of the serum market to trade on
 * @param side The side of the order (ask or bid)
 * @param limitPrice The limit price for the order
 * @param maxQuantityPercentage The percentage (a number from 0 to 100) of the pool assets that should be invested
 * @param orderType For now, all orders are to be set as type ImmediateOrCancel
 * @param clientId The serum clientId for the order, can be set as 0
 * @param selfTradeBehavior The serum self trade behaviour for the order
 * @param srmDiscountKey The address of the srm discount key for the order (optional)
 * @param payerKey The address of the account that should pay for the allocation fees
 * @param amountToTrade If this optional argument is given, it will overwrite the maxQuantityPercentage and fix
 *  the size of the order in base quantity. If you want to trade one and a half FIDA on FIDA/USDC for example,
 *  give 1.5 as an input here.
 */
export async function createOrder(
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
  market: PublicKey,
  side: OrderSide,
  limitPrice: Numberu64,
  maxQuantityPercentage: number,
  orderType: OrderType,
  clientId: Numberu64,
  selfTradeBehavior: SelfTradeBehavior,
  srmDiscountKey: PublicKey | null,
  payerKey: PublicKey,
  amountToTrade?: number,
): Promise<[Account, TransactionInstruction[]]> {

  // Find the pool key
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolInfo.data.slice(0, PoolHeader.LEN),
  );

  let marketData = await getMarketData(connection, market);
  let sourceMintKey: PublicKey;
  let targetMintKey: PublicKey;
  if (side == OrderSide.Ask) {
    sourceMintKey = marketData.coinMintKey;
    targetMintKey = marketData.pcMintKey;
  } else {
    sourceMintKey = marketData.pcMintKey;
    targetMintKey = marketData.coinMintKey;
  }
  console.log('Market key: ', market.toString());

  let authorizedMarkets = unpack_markets(
    poolInfo.data.slice(
      PoolHeader.LEN,
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
    poolHeader.numberOfMarkets,
  );
  let marketIndex = authorizedMarkets
    .map(m => {
      return m.toString();
    })
    .indexOf(market.toString());

  let poolAssets = unpack_assets(
    poolInfo.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let sourcePoolAssetIndex = new Numberu64(
    // @ts-ignore
    poolAssets
      .map(a => {
        return a.mintAddress.toString();
      })
      .indexOf(sourceMintKey.toString()),
  );
  let sourcePoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    sourceMintKey,
  );

  // @ts-ignore
  let targetPoolAssetIndex = poolAssets
    .map(a => {
      return a.mintAddress.toString();
    })
    .indexOf(targetMintKey.toString());

  let createTargetAssetInstruction = undefined;
  if (targetPoolAssetIndex == -1) {
    // Create the target asset account if nonexistent
    let createTargetAssetInstruction = await createAssociatedTokenAccount(
      SystemProgram.programId,
      payerKey,
      poolKey,
      targetMintKey,
    );
    targetPoolAssetIndex = poolAssets.length;
  }

  // Create the open order account with trhe serum specific size of 3228 bits
  let rent = await connection.getMinimumBalanceForRentExemption(3228);
  let openOrderAccount = new Account();
  let openOrderKey = openOrderAccount.publicKey;
  let createAccountParams: CreateAccountParams = {
    fromPubkey: payerKey,
    newAccountPubkey: openOrderKey,
    lamports: rent,
    space: 3228,
    programId: SERUM_PROGRAM_ID,
  };
  let createOpenOrderAccountInstruction = SystemProgram.createAccount(
    createAccountParams,
  );
  console.log('Open Order key: ', openOrderKey.toString());


  // Calcutlate the amount to trade as a ratio fo the pool balance
  let sourcePoolAssetBalance = (await connection.getTokenAccountBalance(sourcePoolAssetKey)).value;
  // @ts-ignore
  let maxQuantityRatioU16 = (!amountToTrade)? new Numberu16(2**16 * maxQuantityPercentage / 100)
  // @ts-ignore
  : new Numberu16((2**16 * amountToTrade / (sourcePoolAssetBalance['uiAmount'])));

  let createOrderTxInstruction = createOrderInstruction(
    BONFIDABOT_PROGRAM_ID,
    poolHeader.signalProvider,
    market,
    sourcePoolAssetKey,
    sourcePoolAssetIndex,
    // @ts-ignore
    new Numberu64(targetPoolAssetIndex),
    openOrderKey,
    marketData.reqQueueKey,
    marketData.eventQueueKey,
    marketData.bidsKey,
    marketData.asksKey,
    poolKey,
    marketData.coinVaultKey,
    marketData.pcVaultKey,
    TOKEN_PROGRAM_ID,
    SERUM_PROGRAM_ID,
    SYSVAR_RENT_PUBKEY,
    srmDiscountKey,
    [poolSeed],
    side,
    limitPrice,
    // @ts-ignore
    new Numberu16(marketIndex),
    marketData.coinLotSize,
    marketData.pcLotSize,
    targetMintKey,
    maxQuantityRatioU16,
    orderType,
    clientId,
    selfTradeBehavior,
    // @ts-ignore
    new Numberu16((1<<16) - 1)
  );

  let instructions = [
    createOpenOrderAccountInstruction,
    createOrderTxInstruction,
  ];
  if (!!createTargetAssetInstruction) {
    instructions.unshift(createTargetAssetInstruction);
  }
  return [openOrderAccount, instructions];
}


/**
 * Returns the solana instructions to a crank to settle funds out of one of the pool's active OpenOrders accounts.
 * (Permissionless)
 *
 * @param connection The connection object to the rpc node
 * @param poolSeed The seed of the pool that should be settled
 * @param market The address of the serum market on which the order is
 * @param openOrdersKey The address of the serum openOrder account to settle
 * @param srmReferrerKey The address of the referer that should receive the serum trading fees
 */
export async function settleFunds(
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
  market: PublicKey,
  openOrdersKey: PublicKey,
  srmReferrerKey: PublicKey | null,
): Promise<TransactionInstruction[]> {
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    BONFIDABOT_PROGRAM_ID,
  );
  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }

  let marketData = await getMarketData(connection, market);
  let poolHeader = PoolHeader.fromBuffer(
    poolInfo.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolInfo.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let coinPoolAssetIndex = new Numberu64(
    // @ts-ignore
    poolAssets
      .map(a => {
        return a.mintAddress.toString();
      })
      .indexOf(marketData.coinMintKey.toString()),
  );
  let coinPoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    marketData.coinMintKey,
  );

  let pcPoolAssetIndex = new Numberu64(
    // @ts-ignore
    poolAssets
      .map(a => {
        return a.mintAddress.toString();
      })
      .indexOf(marketData.pcMintKey.toString()),
  );
  let pcPoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    marketData.pcMintKey,
  );

  let vaultSignerKey = await PublicKey.createProgramAddress(
    [market.toBuffer(), marketData.vaultSignerNonce.toBuffer()],
    SERUM_PROGRAM_ID,
  );

  let settleFundsTxInstruction = settleFundsInstruction(
    BONFIDABOT_PROGRAM_ID,
    market,
    openOrdersKey,
    poolKey,
    poolMintKey,
    marketData.coinVaultKey,
    marketData.pcVaultKey,
    coinPoolAssetKey,
    pcPoolAssetKey,
    vaultSignerKey,
    TOKEN_PROGRAM_ID,
    SERUM_PROGRAM_ID,
    srmReferrerKey,
    [poolSeed],
    pcPoolAssetIndex,
    coinPoolAssetIndex,
  );

  return [settleFundsTxInstruction];
}

/**
 * This method is obsolete for now as all orders can only be passed as ImmediateOrCancel
 *
 * @param connection
 * @param poolSeed
 * @param market
 * @param openOrdersKey
 */
export async function cancelOrder(
  connection: Connection,
  poolSeed: Buffer | Uint8Array,
  market: PublicKey,
  openOrdersKey: PublicKey,
): Promise<TransactionInstruction[]> {
  // Find the pool key
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    BONFIDABOT_PROGRAM_ID,
  );

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let signalProviderKey = PoolHeader.fromBuffer(
    poolInfo.data.slice(0, PoolHeader.LEN),
  ).signalProvider;
  let marketData = await getMarketData(connection, market);

  let openOrders = await OpenOrders.load(
    connection,
    openOrdersKey,
    SERUM_PROGRAM_ID,
  );
  let orders = openOrders.orders;

  // @ts-ignore
  let orderId: Numberu128 = new Numberu128(orders[0].toBuffer());

  // @ts-ignore
  if (orderId == new Numberu128(0)) {
    throw 'No orders found in Openorder account.';
  }

  let side = 1 - orderId.toBuffer()[7];

  let cancelOrderTxInstruction = await cancelOrderInstruction(
    BONFIDABOT_PROGRAM_ID,
    signalProviderKey,
    market,
    openOrdersKey,
    marketData.eventQueueKey,
    marketData.bidsKey,
    marketData.asksKey,
    poolKey,
    SERUM_PROGRAM_ID,
    [poolSeed],
    side,
    orderId,
  );

  return [cancelOrderTxInstruction];
}


/**
 * Returns the solana instructions to buy out of the pool by redeeming (burning) pooltokens.
 * This instruction needs to be executed after (and within the same transaction)
 * having settled on all possible open orders for the pool.
 * This is because as long as an order is open for the pool, redeeming is impossible.
 * (Signed by the owner of the pooltokens)
 *
 * @param connection The connection object to the rpc node
 * @param sourcePoolTokenOwnerKey The address of the account that owns the pooltokens to be redeemed
 * @param sourcePoolTokenKey The address that holds the pooltokens
 * @param targetAssetKeys An array of addresses to which the pool asset tokens are payed out to
 * @param poolSeed The seed of the pool that should be redeemed from
 * @param poolTokenAmount The amount of pooltokens that should be used (ie the amount of tokens that should be bought back)
 */
export async function redeem(
  connection: Connection,
  sourcePoolTokenOwnerKey: PublicKey,
  sourcePoolTokenKey: PublicKey,
  targetAssetKeys: Array<PublicKey>,
  poolSeed: Array<Buffer | Uint8Array>,
  poolTokenAmount: Numberu64,
): Promise<TransactionInstruction[]> {

  // Find the pool key and mint key
  let poolKey = await PublicKey.createProgramAddress(
    poolSeed,
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    poolSeed.concat(array_one),
    BONFIDABOT_PROGRAM_ID,
  );

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolData = poolInfo.data;
  let poolHeader = PoolHeader.fromBuffer(poolData.slice(0, PoolHeader.LEN));
  let poolAssets = unpack_assets(
    poolInfo.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );
  let poolAssetKeys: Array<PublicKey> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    poolAssetKeys.push(assetKey);
  }

  let redeemTxInstruction = redeemInstruction(
    TOKEN_PROGRAM_ID,
    BONFIDABOT_PROGRAM_ID,
    SYSVAR_CLOCK_PUBKEY,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    sourcePoolTokenOwnerKey,
    sourcePoolTokenKey,
    targetAssetKeys,
    poolSeed,
    poolTokenAmount,
  );
  return [redeemTxInstruction];
}


 /**
  *  Returns the solana instructions to collect the fees from the pool.
  *  See the readme for the payout destinations.
  * (Permissionless)
  *
  * @param connection The connection object to the rpc node
  * @param poolSeed The seed of the pool
  */
export async function collectFees(
  connection: Connection,
  poolSeed: Array<Buffer | Uint8Array>,
): Promise<TransactionInstruction[]> {
  // Find the pool key and mint key
  let poolKey = await PublicKey.createProgramAddress(
    poolSeed,
    BONFIDABOT_PROGRAM_ID,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    poolSeed.concat(array_one),
    BONFIDABOT_PROGRAM_ID,
  );

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolData = poolInfo.data;
  let poolHeader = PoolHeader.fromBuffer(poolData.slice(0, PoolHeader.LEN));

  let sigProviderFeeReceiverKey = await findAssociatedTokenAddress(
    poolHeader.signalProvider,
    poolMintKey,
  );
  let bonfidaFeeReceiverKey = await findAssociatedTokenAddress(
    BONFIDA_FEE_KEY,
    poolMintKey,
  );
  let bonfidaBuyAndBurnKey = await findAssociatedTokenAddress(
    BONFIDA_BNB_KEY,
    poolMintKey,
  );

  let collectFeesTxInstruction = collectFeesInstruction(
    TOKEN_PROGRAM_ID,
    SYSVAR_CLOCK_PUBKEY,
    BONFIDABOT_PROGRAM_ID,
    poolKey,
    poolMintKey,
    sigProviderFeeReceiverKey,
    bonfidaFeeReceiverKey,
    bonfidaBuyAndBurnKey,
    poolSeed,
  );
  return [collectFeesTxInstruction];
}
