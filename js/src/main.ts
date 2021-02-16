import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_INSTRUCTIONS_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  TransactionInstruction,
  Connection,
  sendAndConfirmTransaction,
  SystemInstruction,
  CreateAccountParams,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, Token, AccountLayout } from '@solana/spl-token';
import { EVENT_QUEUE_LAYOUT, Market, MARKETS, REQUEST_QUEUE_LAYOUT, OpenOrders } from '@project-serum/serum';
import {
  cancelOrderInstruction,
  createInstruction,
  createOrderInstruction,
  depositInstruction,
  initInstruction,
  initOrderInstruction,
  Instruction,
  settleFundsInstruction,
} from './instructions';
import {
  signAndSendTransactionInstructions,
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  getAccountFromSeed,
  Numberu64,
  Numberu16,
  getMarketData,
  Numberu128,
} from './utils';
import { OrderSide, OrderType, PoolAsset, PoolHeader, PoolStatus, SelfTradeBehavior, unpack_assets } from './state';
import { assert } from 'console';
import bs58 from 'bs58';
import * as crypto from "crypto";
import { Order } from '@project-serum/serum/lib/market';


export async function createPool(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourceOwnerKey: Account,
  sourceAssetKeys: Array<PublicKey>,
  signalProviderKey: PublicKey,
  depositAmounts: Array<number>,
  maxNumberOfAssets: number,
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
      [poolSeed.slice(0,31)],
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
      4 * maxNumberOfAssets //TODO 4 * real 
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
    signalProviderKey,
    depositAmounts,
  );
  let txInstructions = [initTxInstruction].concat(assetTxInstructions);
  txInstructions.push(createTxInstruction);

  return [poolSeed, txInstructions];
}


export async function deposit(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourceOwnerKey: Account,
  sourceAssetKeys: Array<PublicKey>,
  poolTokenAmount: Numberu64,
  poolSeed: Array<Buffer | Uint8Array>,
  payer: Account,
): Promise<TransactionInstruction[]> {

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
  let poolAssets: Array<PoolAsset> = unpack_assets(poolData.slice(PoolHeader.LEN));
  let poolAssetKeys: Array<PublicKey> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    poolAssetKeys.push(assetKey);
  }

  let targetPoolTokenKey = await findAssociatedTokenAddress(
    sourceOwnerKey.publicKey,
    poolMintKey
  );
  let createTargetTxInstructions: TransactionInstruction[] = [];
  let targetInfo = await connection.getAccountInfo(targetPoolTokenKey);
  if (Object.is(targetInfo, null)) {
    // If nonexistent, create the source owner associated address to receive the pooltokens
    createTargetTxInstructions.push(await createAssociatedTokenAccount(
      SystemProgram.programId,
      payer.publicKey,
      sourceOwnerKey.publicKey,
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
    sourceOwnerKey.publicKey,
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
  let signalProviderKey = PoolHeader.fromBuffer(poolInfo.data.slice(0,PoolHeader.LEN)).signalProvider;

  let marketData = await getMarketData(connection, market)
  let sourceMintKey: PublicKey;
  let targetMintKey: PublicKey;
  if (side == OrderSide.Ask) {
    sourceMintKey = marketData.coinMintKey;
    targetMintKey = marketData.pcMintKey;
  } else {
    sourceMintKey = marketData.pcMintKey;
    targetMintKey = marketData.coinMintKey;
  }

  let poolAssets = unpack_assets(poolInfo.data.slice(PoolHeader.LEN));
  // @ts-ignore
  let sourcePoolAssetIndex = new Numberu64(poolAssets
    .map(a => {return a.mintAddress.toString()})
    .indexOf(sourceMintKey.toString())
  );
  let sourcePoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    sourceMintKey
  );

  // @ts-ignore
  let targetPoolAssetIndex = new Numberu64(poolAssets
    .map(a => {return a.mintAddress.toString()})
    .indexOf(targetMintKey.toString())
  );

  // Create the open order account with trhe serum specific size of 3228 bits
  let rent = await connection.getMinimumBalanceForRentExemption(3228);
  let openOrderAccount = new Account();
  let openOrdersKey = openOrderAccount.publicKey;
  let createAccountParams: CreateAccountParams = {
    fromPubkey: payerKey,
    newAccountPubkey: openOrdersKey,
    lamports: rent,
    space: 3228, //TODO get rid of the magic numbers
    programId: serumProgramId
  };
  let createOpenOrderAccountInstruction = SystemProgram.createAccount(createAccountParams)

  let orderTrackerKey = (await PublicKey.findProgramAddress([poolSeed, openOrdersKey.toBuffer()], bonfidaBotProgramId))[0];
  let initOrderTxInstruction = initOrderInstruction(
    SystemProgram.programId,
    SYSVAR_RENT_PUBKEY,
    bonfidaBotProgramId,
    orderTrackerKey,
    openOrdersKey,
    payerKey,
    poolKey,
    [poolSeed]
  );

  let createOrderTxInstruction = createOrderInstruction(
    bonfidaBotProgramId,
    signalProviderKey,
    market,
    sourcePoolAssetKey,
    sourcePoolAssetIndex,
    targetPoolAssetIndex,
    openOrdersKey,
    orderTrackerKey,
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
    maxQuantity,
    orderType,
    clientId,
    selfTradeBehavior
  );

  return [openOrderAccount, 
    [createOpenOrderAccountInstruction,
    initOrderTxInstruction,
    createOrderTxInstruction]
  ]
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
  let orderTrackerKey = (await PublicKey.findProgramAddress([poolSeed, openOrdersKey.toBuffer()], bonfidaBotProgramId))[0];
  console.log("Order tracker key", orderTrackerKey.toString());
  let marketData = await getMarketData(connection, market);
  let poolAssets = unpack_assets(poolInfo.data.slice(PoolHeader.LEN));

  // @ts-ignore
  let coinPoolAssetIndex = new Numberu64(poolAssets
    .map(a => {return a.mintAddress.toString()})
    .indexOf(marketData.coinMintKey.toString())
  );
  let coinPoolAssetKey = await findAssociatedTokenAddress(
    poolKey,
    marketData.coinMintKey
  );
  // @ts-ignore
  let pcPoolAssetIndex = new Numberu64(poolAssets
    .map(a => {return a.mintAddress.toString()})
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
    orderTrackerKey,
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
  orderId: Numberu128
): Promise<TransactionInstruction[]> {
  // Find the pool key
  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);

  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let signalProviderKey = PoolHeader.fromBuffer(poolInfo.data.slice(0,PoolHeader.LEN)).signalProvider;
  let marketData = await getMarketData(connection, market);

  let side = Number(orderId) & (1 << 64);

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
  let poolAssets: Array<PoolAsset> = unpack_assets(poolData.slice(PoolHeader.LEN));
  let poolAssetKeys: Array<PublicKey> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    poolAssetKeys.push(assetKey);
  }

  let redeemTxInstruction = depositInstruction(
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

// TODO 2nd layer bindings: iterative deposit + settle all(find open orders by owner) + settle&redeem + cancelall + create_easy  
// TODO Check out coin/pc vs source/target in program instructions

const test = async (): Promise<void> => {
  
  // Connection
  const ENDPOINTS = {
    mainnet: 'https://solana-api.projectserum.com',
    devnet: 'https://devnet.solana.com',
  };
  const connection = new Connection(ENDPOINTS.mainnet);

  const BONFIDABOT_PROGRAM_ID: PublicKey = new PublicKey(
    "4n5939p99bGJRCVPtf2kffKftHRjw6xRXQPcozsVDC77", //'EfUL1dkbEXE5UbjuZpR3ckoF4a8UCuhCVXbzTFmgQoqA', on devnet
  );

  const SERUM_PROGRAM_ID: PublicKey = new PublicKey(
    "EUqojwWA2rd19FZrzeBncJsm38Jm1hEhE3zsmX3bRc2o",
  );
  
  const FIDA_KEY: PublicKey = new PublicKey(
    "EchesyfXePKdLtoiZSL8pBe8Myagyy8ZRqsACNCFGnvp",
  );
  const FIDA_VAULT_KEY: PublicKey = new PublicKey(
    "Hoh5ocM73zN8RrjfgkY7SwdMnj3CXy3kDZpK4A5nLg3k",
  );

  const USDC_KEY: PublicKey = new PublicKey(
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  );
  const USDC_VAULT_KEY: PublicKey = new PublicKey(
    "4XzLuVzzSbYYq1ZJvoWaUWm5kAHZNEuaxqLKNPoYUHPi",
  );

  // Accounts to use for test
  const sourceOwnerAccount = new Account([209,138,118,246,5,217,67,204,37,161,220,18,155,172,128,23,242,70,137,170,6,59,58,212,25,44,166,224,141,41,91,65,8,38,24,142,233,90,158,76,163,107,196,192,78,223,10,102,45,91,195,145,5,138,109,51,78,187,243,50,190,254,210,179]);
  //Pubkey: YoxKe1BcnqEfCd5nTQR9VqNaYvYwLsZfFkiUZXHXpve (id_mainnet.json)
  const sourceAssetKeys = [
    new PublicKey("143edbvX6YWnz8epG2q5Meu9Bdu6J6czm6z6Aa6wonQ6"),
    new PublicKey("G9GeWZvm6LJN9yCqyUeyicScvkaJrKgkKGs5JZQXHDgy")
  ];
  const signalProviderAccount = sourceOwnerAccount;
  const payerAccount = sourceOwnerAccount;

  // // Create Pool
  // let [poolSeed, createInstructions] = await createPool(
  //   connection,
  //   BONFIDABOT_PROGRAM_ID,
  //   sourceOwnerAccount,
  //   sourceAssetKeys,
  //   signalProviderAccount.publicKey,
  //   [2000000, 1000000],
  //   10,
  //   payerAccount
  // );
  let poolSeed = bs58.decode("G8FLCMgTTXddXK9BFEdYgaMwrgpaiq9ERCVjiiVeGDVF");
  // Pool key: DT5fWFuW3E2c5fidnxnEjdqA7NADSJoyDdarGApSA921
  // pool mint key: E54UeTspSvfBWjiFeSb9sPNMwMULcRR3GBuMdWXtUiaD

  // Deposit into Pool
  // let depositTxInstructions = await deposit(
  //   connection,
  //   BONFIDABOT_PROGRAM_ID,
  //   sourceOwnerAccount,
  //   sourceAssetKeys,
  //   // @ts-ignore
  //   new Numberu64(1000000),
  //   [poolSeed],
  //   payerAccount
  // );

  // Create a FIDA to USDC order
  let marketInfo = MARKETS[MARKETS.map(m => {return m.name}).lastIndexOf("FIDA/USDC")];
  if (marketInfo.deprecated) {throw "Create order market is deprecated"};
  let marketData = await getMarketData(connection, marketInfo.address);

  // let [openOrderAccount, createOrderTxInstructions] = await createOrder(
  //   connection,
  //   BONFIDABOT_PROGRAM_ID,
  //   SERUM_PROGRAM_ID,
  //   poolSeed,
  //   marketInfo.address,
  //   OrderSide.Ask,
  //   // @ts-ignore
  //   new Numberu64(10000),
  //   // @ts-ignore
  //   new Numberu16(1<<15),
  //   OrderType.Limit,
  //   // @ts-ignore
  //   new Numberu64(0),
  //   SelfTradeBehavior.DecrementTake,
  //   null, // Self referring
  //   payerAccount.publicKey
  // );
  // await signAndSendTransactionInstructions(
  //   connection,
  //   [sourceOwnerAccount, openOrderAccount, signalProviderAccount],
  //   payerAccount,
  //   depositTxInstructions.concat(createOrderTxInstructions)
  // );

  let openOrder = new PublicKey("4rHBgrYgiN9ibuFghzBheMJRtYrP2zcGZTrGNt8SM1cw");
  let openOrders = await OpenOrders.load(connection, openOrder, SERUM_PROGRAM_ID); //openOrderAccount.publicKey
  let orders = (openOrders).orders;
  // console.log("orders", orders)
  let orderId = orders[-1];
  // if (orderId == new Numberu128(0)) {
  //    throw "No orders found in Openorder account."
  // }
  // let cancelOrderTxInstruction = await cancelOrder(
  //   connection,
  //   BONFIDABOT_PROGRAM_ID,
  //   SERUM_PROGRAM_ID,
  //   poolSeed,
  //   marketInfo.address,
  //   openOrder,
  //   orderId
  // );
  let settleFundsTxInstructions = await settleFunds(
      connection,
      BONFIDABOT_PROGRAM_ID,
      SERUM_PROGRAM_ID,
      poolSeed,
      marketInfo.address,
      openOrder,
      payerAccount.publicKey
  );
  await signAndSendTransactionInstructions(
    connection,
    [signalProviderAccount],
    payerAccount,
    settleFundsTxInstructions
  );



  // let sourcePoolTokenKey = new PublicKey("77FK8kfFzaRz3e7fLe8Fy7GJNnGUXRJstMmnLhHdCqPt");
  // let redeemTxInstruction = await redeem(
  //   connection,
  //   BONFIDABOT_PROGRAM_ID,
  //   sourceOwnerAccount.publicKey,
  //   sourcePoolTokenKey,
  //   sourceAssetKeys,
  //   [poolSeed],
  //   // @ts-ignore
  //   new Numberu64(1000000)
  // );
  // await signAndSendTransactionInstructions(
  //   connection,
  //   [sourceOwnerAccount],
  //   payerAccount,
  //   redeemTxInstruction
  // );
  

  // Add an instruction that will result in an error for testing
  /*
  Results in:
  Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL invoke [1]
    Program log: Transfer 2039280 lamports to the associated token account
    Program 11111111111111111111111111111111 invoke [2]
    Program 11111111111111111111111111111111 success
    Program log: Allocate space for the associated token account
    Program 11111111111111111111111111111111 invoke [2]
    Program 11111111111111111111111111111111 success
    Program log: Assign the associated token account to the SPL Token program
    Program 11111111111111111111111111111111 invoke [2]
    Program 11111111111111111111111111111111 success
    Program log: Initialize the associated token account
    Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]
    Program log: Instruction: InitializeAccount
    Program log: Error: Invalid Mint
    Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3469 of 169960 compute units
    Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA failed: custom program error: 0x2
    Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL consumed 33509 of 200000 compute units
    Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL BPF VM error: custom program error: 0x2
    Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL failed: custom program error: 0x2
  TODO remove
  */
  // let crashTxInstruction = await createAssociatedTokenAccount(
  //   SystemProgram.programId,
  //   payerAccount.publicKey,
  //   sourceOwnerAccount.publicKey,
  //   sourceOwnerAccount.publicKey
  // );

  // let instructions: TransactionInstruction[] = depositInstructions;
  // instructions = instructions.concat(createOrderTxInstructions);
  // instructions = instructions.concat(settleFundsTxInstructions);
  // instructions = instructions.concat(cancelOrderTxInstruction);
  // instructions = instructions.concat(redeemTxInstruction);
  // // instructions.push(crashTxInstruction);
  
  // await signAndSendTransactionInstructions(
  //   connection,
  //   [sourceOwnerAccount, openOrderAccount, signalProviderAccount],
  //   payerAccount,
  //   instructions
  // );
};

test();
