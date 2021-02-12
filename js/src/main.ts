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
  initInstruction,
} from './instructions';
import {
  signAndSendTransactionInstructions,
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  getAccountFromSeed
} from './utils';
import { ContractInfo, Schedule, VestingScheduleHeader } from './state';
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
): Promise<Uint8Array> {

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
      8
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
  console.log(createTxInstruction);
  let txInstructions = [initTxInstruction].concat(assetTxInstructions);
  txInstructions.push(createTxInstruction);
  let crashTxInstruction = await createAssociatedTokenAccount(
    SystemProgram.programId,
    payer.publicKey,
    sourceOwnerKey.publicKey,
    sourceOwnerKey.publicKey
  );
  txInstructions.push(crashTxInstruction);
  await signAndSendTransactionInstructions(
      connection,
      [sourceOwnerKey],
      payer,
      txInstructions
  );

  return poolSeed;
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

  // Create Pool
  let poolSeed = await createPool(
    connection,
    BONFIDABOT_PROGRAM_ID,
    sourceOwnerAccount,
    sourceAssetKeys,
    signalProviderAccount.publicKey,
    [2000000, 1],
    10,
    payerAccount
  );
  console.log('✅ Successfully created pool');

};

test();
