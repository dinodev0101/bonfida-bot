import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  SYSVAR_CLOCK_PUBKEY,
  TransactionInstruction,
  Connection,
  sendAndConfirmTransaction,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, Token, AccountLayout } from '@solana/spl-token';
import {
  createInstruction,
  depositInstruction,
  initInstruction,
  Instruction,
} from './instructions';
import {
  signAndSendTransactionInstructions,
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  getAccountFromSeed
} from './utils';
import { PoolAsset, PoolHeader, PoolStatus, unpack_assets } from './state';
import { assert } from 'console';
import bs58 from 'bs58';
import * as crypto from "crypto";


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
      8 // 4 * real
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
  poolTokenAmount: number,
  poolSeed: Array<Buffer | Uint8Array>,
  payer: Account,
): Promise<TransactionInstruction[]> {

  // Find a valid pool seed
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
  let depositInstructions = await deposit(
    connection,
    BONFIDABOT_PROGRAM_ID,
    sourceOwnerAccount,
    sourceAssetKeys,
    1000000,
    [poolSeed],
    payerAccount
  );

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
  let crashTxInstruction = await createAssociatedTokenAccount(
    SystemProgram.programId,
    payerAccount.publicKey,
    sourceOwnerAccount.publicKey,
    sourceOwnerAccount.publicKey
  );

  let instructions: TransactionInstruction[] = depositInstructions;//createInstructions;
  // instructions = instructions.concat(depositInstructions);
  instructions.push(crashTxInstruction);
  
  await signAndSendTransactionInstructions(
    connection,
    [sourceOwnerAccount],
    payerAccount,
    instructions
  );
};

test();
