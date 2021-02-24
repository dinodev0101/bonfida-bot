import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  TransactionInstruction,
  Connection,
  CreateAccountParams,
  InstructionType,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, AccountLayout } from '@solana/spl-token';
import {
  cancelOrderInstruction,
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
} from './utils';
import { OrderSide, OrderType, PoolAsset, PoolHeader, SelfTradeBehavior, unpack_assets, PUBKEY_LENGTH, unpack_markets } from './state';
import bs58 from 'bs58';
import * as crypto from "crypto";
import { open } from 'fs/promises';
import { OpenOrders } from '@project-serum/serum';


/////////////////////////////////

export const ENDPOINTS = {
  mainnet: 'https://solana-api.projectserum.com',
  devnet: 'https://devnet.solana.com',
};

export const BONFIDABOT_PROGRAM_ID: PublicKey = new PublicKey(
  "GCv8mMWTwpYCNh6xbMPsx2Z7yKrjCC7LUz6nd3cMZokB",
);

export const SERUM_PROGRAM_ID: PublicKey = new PublicKey(
  "EUqojwWA2rd19FZrzeBncJsm38Jm1hEhE3zsmX3bRc2o",
);

export const FIDA_KEY: PublicKey = new PublicKey(
  "EchesyfXePKdLtoiZSL8pBe8Myagyy8ZRqsACNCFGnvp",
);

export const PUBLIC_POOLS_SEEDS = [
  new PublicKey("5xK9ByTt1MXP6SfB9BXL16GLRdsCqNr8Xj1SToje12Sa"),
];

/////////////////////////////////


export async function createPool(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  serumProgramId: PublicKey,
  sourceOwnerKey: Account,
  sourceAssetKeys: Array<PublicKey>,
  signalProviderKey: PublicKey,
  depositAmounts: Array<number>,
  maxNumberOfAssets: number,
  number_of_markets: Numberu16,
  markets: Array<PublicKey>,
  payer: Account,
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
      bonfidaBotProgramId,
    );
    poolSeed[31] = bump;
    try {
      await PublicKey.createProgramAddress([poolSeed, array_one], bonfidaBotProgramId);
      break;
    } catch (e) {
      continue;
    }
  }
  let poolMintKey = await PublicKey.createProgramAddress([poolSeed, array_one], bonfidaBotProgramId);
  console.log('Pool seed: ', bs58.encode(poolSeed));
  console.log('Pool key: ', poolKey.toString());
  console.log('Mint key: ', poolMintKey.toString());


  // Initialize the pool
  let initTxInstruction = initInstruction(
    TOKEN_PROGRAM_ID,
    SystemProgram.programId,
    SYSVAR_RENT_PUBKEY,
    bonfidaBotProgramId,
    poolMintKey,
    payer.publicKey,
    poolKey,
    [poolSeed],
    maxNumberOfAssets,
    number_of_markets
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
    assetTxInstructions.push(await createAssociatedTokenAccount(
      SystemProgram.programId,
      payer.publicKey,
      poolKey,
      assetMint
    ));
    poolAssetKeys.push(await findAssociatedTokenAddress(
      poolKey,
      assetMint
    ));
  }
  // Create the source owner associated address to receive the pooltokens
  assetTxInstructions.push(await createAssociatedTokenAccount(
    SystemProgram.programId,
    payer.publicKey,
    sourceOwnerKey.publicKey,
    poolMintKey
  ));
  let targetPoolTokenKey = await findAssociatedTokenAddress(
    sourceOwnerKey.publicKey,
    poolMintKey
  );

  // Create the pool
  let createTxInstruction = createInstruction(
    TOKEN_PROGRAM_ID,
    bonfidaBotProgramId,
    poolMintKey,
    poolKey,
    [poolSeed],
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwnerKey.publicKey,
    sourceAssetKeys,
    serumProgramId,
    signalProviderKey,
    depositAmounts,
    markets
  );
  let txInstructions = [initTxInstruction].concat(assetTxInstructions);
  txInstructions.push(createTxInstruction);

  return [poolSeed, txInstructions];
}


export async function deposit(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  poolTokenAmount: Numberu64,
  poolSeed: Array<Buffer | Uint8Array>,
  payer: Account,
): Promise<TransactionInstruction[]> {

  //TODO Collect fees beforehand

  // Find the pool key and mint key
  let poolKey = await PublicKey.createProgramAddress(poolSeed, bonfidaBotProgramId);
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(poolSeed.concat(array_one), bonfidaBotProgramId);

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolData = poolInfo.data;
  let poolHeader = PoolHeader.fromBuffer(poolData.slice(0, PoolHeader.LEN));
  let poolAssets: Array<PoolAsset> = unpack_assets(
    poolData.slice(PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH)
  );

  let poolAssetKeys: Array<PublicKey> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    poolAssetKeys.push(assetKey);
  }

  let targetPoolTokenKey = await findAssociatedTokenAddress(
    sourceOwnerKey,
    poolMintKey
  );
  let createTargetTxInstructions: TransactionInstruction[] = [];
  let targetInfo = await connection.getAccountInfo(targetPoolTokenKey);
  if (Object.is(targetInfo, null)) {
    // If nonexistent, create the source owner associated address to receive the pooltokens
    createTargetTxInstructions.push(await createAssociatedTokenAccount(
      SystemProgram.programId,
      payer.publicKey,
      sourceOwnerKey,
      poolMintKey
    ));
  }

  let depositTxInstruction = depositInstruction(
    TOKEN_PROGRAM_ID,
    bonfidaBotProgramId,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwnerKey,
    sourceAssetKeys,
    poolSeed,
    poolTokenAmount
  )
  return createTargetTxInstructions.concat(depositTxInstruction)
}

export async function createOrder(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  serumProgramId: PublicKey,
  poolSeed: Buffer | Uint8Array,
  market: PublicKey,
  side: OrderSide,
  limitPrice: Numberu64,
  maxQuantity: Numberu16,
  orderType: OrderType,
  clientId: Numberu64,
  selfTradeBehavior: SelfTradeBehavior,
  srmReferrerKey: PublicKey | null,
  payerKey: PublicKey
): Promise<[Account, TransactionInstruction[]]> {
  // Find the pool key
  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(poolInfo.data.slice(0, PoolHeader.LEN));

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

  let authorizedMarkets = unpack_markets(poolInfo.data.slice(
    PoolHeader.LEN, PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
  ), poolHeader.numberOfMarkets);
  let marketIndex = authorizedMarkets.map(m => {return m.toString()}).indexOf(market.toString());

  let poolAssets = unpack_assets(poolInfo.data.slice(PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH));
  // @ts-ignore
  let sourcePoolAssetIndex = new Numberu64(poolAssets
    .map(a => { return a.mintAddress.toString() })
    .indexOf(sourceMintKey.toString())
  );
  let sourcePoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    sourceMintKey
  );

  // @ts-ignore
  let targetPoolAssetIndex = poolAssets
    .map(a => { return a.mintAddress.toString() })
    .indexOf(targetMintKey.toString());
    

  let createTargetAssetInstruction = undefined;
  if (targetPoolAssetIndex == -1) {
    // Create the target asset account if nonexistent
    let createTargetAssetInstruction = await createAssociatedTokenAccount(
      SystemProgram.programId,
      payerKey,
      poolKey,
      targetMintKey
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
    space: 3228, //TODO get rid of the magic numbers
    programId: serumProgramId
  };
  let createOpenOrderAccountInstruction = SystemProgram.createAccount(createAccountParams);
  console.log('Open Order key: ', openOrderKey.toString());


  let createOrderTxInstruction = createOrderInstruction(
    bonfidaBotProgramId,
    poolHeader.signalProvider,
    market,
    sourcePoolAssetKey,
    sourcePoolAssetIndex,
    // @ts-ignore
    new Numberu64(targetPoolAssetIndex),
    openOrderKey,
    marketData.reqQueueKey,
    poolKey,
    marketData.coinVaultKey,
    marketData.pcVaultKey,
    TOKEN_PROGRAM_ID,
    serumProgramId,
    SYSVAR_RENT_PUBKEY,
    srmReferrerKey,
    [poolSeed],
    side,
    limitPrice,
    // @ts-ignore
    new Numberu16(marketIndex),
    marketData.coinLotSize,
    marketData.pcLotSize,
    targetMintKey,
    maxQuantity,
    orderType,
    clientId,
    selfTradeBehavior,
  );
  
  let instructions = [createOpenOrderAccountInstruction,
    createOrderTxInstruction
  ];
  if (!!createTargetAssetInstruction) {
    instructions.unshift(createTargetAssetInstruction);
  }
  return [openOrderAccount, instructions]
}

export async function settleFunds(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  dexProgramKey: PublicKey,
  poolSeed: Buffer | Uint8Array,
  market: PublicKey,
  openOrdersKey: PublicKey,
  srmReferrerKey: PublicKey | null,
): Promise<TransactionInstruction[]> {

  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress([poolSeed, array_one], bonfidaBotProgramId);
  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }

  let marketData = await getMarketData(connection, market);
  let poolHeader = PoolHeader.fromBuffer(poolInfo.data.slice(0, PoolHeader.LEN));
  let poolAssets = unpack_assets(poolInfo.data.slice(
    PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
  ));

  // @ts-ignore
  let coinPoolAssetIndex = new Numberu64(poolAssets
    .map(a => { return a.mintAddress.toString() })
    .indexOf(marketData.coinMintKey.toString())
  );
  let coinPoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    marketData.coinMintKey
  );
  // @ts-ignore
  let pcPoolAssetIndex = new Numberu64(poolAssets
    .map(a => { return a.mintAddress.toString() })
    .indexOf(marketData.pcMintKey.toString())
  );
  let pcPoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    marketData.pcMintKey
  );

  let vaultSignerKey = await PublicKey.createProgramAddress(
    [market.toBuffer(), marketData.vaultSignerNonce.toBuffer()],
    dexProgramKey
  );

  let settleFundsTxInstruction = settleFundsInstruction(
    bonfidaBotProgramId,
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
    dexProgramKey,
    srmReferrerKey,
    [poolSeed],
    pcPoolAssetIndex,
    coinPoolAssetIndex,
  )

  return [settleFundsTxInstruction]
}

export async function cancelOrder(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  dexProgramKey: PublicKey,
  poolSeed: Buffer | Uint8Array,
  market: PublicKey,
  openOrdersKey: PublicKey,
): Promise<TransactionInstruction[]> {
  // Find the pool key
  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let signalProviderKey = PoolHeader.fromBuffer(poolInfo.data.slice(0, PoolHeader.LEN)).signalProvider;
  let marketData = await getMarketData(connection, market);

  let openOrders = await OpenOrders.load(connection, openOrdersKey, dexProgramKey);
  let orders = (openOrders).orders;
  
  // @ts-ignore
  let orderId: Numberu128 = new Numberu128(orders[0].toBuffer());

  // @ts-ignore
  if (orderId == new Numberu128(0)) {
     throw "No orders found in Openorder account."
  }

  let side = 1 - orderId.toBuffer()[7];

  let cancelOrderTxInstruction = await cancelOrderInstruction(
    bonfidaBotProgramId,
    signalProviderKey,
    market,
    openOrdersKey,
    marketData.reqQueueKey,
    poolKey,
    dexProgramKey,
    [poolSeed],
    side,
    orderId
  );

  return [cancelOrderTxInstruction];
}

export async function redeem(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourcePoolTokenOwnerKey: PublicKey,
  sourcePoolTokenKey: PublicKey,
  targetAssetKeys: Array<PublicKey>,
  poolSeed: Array<Buffer | Uint8Array>,
  poolTokenAmount: Numberu64,
): Promise<TransactionInstruction[]> {

  // TODO collect fees if necessary

  // Find the pool key and mint key
  let poolKey = await PublicKey.createProgramAddress(poolSeed, bonfidaBotProgramId);
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(poolSeed.concat(array_one), bonfidaBotProgramId);

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolData = poolInfo.data;
  let poolHeader = PoolHeader.fromBuffer(poolData.slice(0, PoolHeader.LEN));
  let poolAssets = unpack_assets(
    poolInfo.data.slice(PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH)
  );
  let poolAssetKeys: Array<PublicKey> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    poolAssetKeys.push(assetKey);
  }

  let redeemTxInstruction = redeemInstruction(
    TOKEN_PROGRAM_ID,
    bonfidaBotProgramId,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    sourcePoolTokenOwnerKey,
    sourcePoolTokenKey,
    targetAssetKeys,
    poolSeed,
    poolTokenAmount,
  )
  return [redeemTxInstruction]
}