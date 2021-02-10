import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_CLOCK_PUBKEY,
  TransactionInstruction,
  Connection,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';
import {
  InitInstruction,
} from './instructions';
import {
  connection,
  account,
  VESTING_PROGRAM_ID,
  tokenPubkey,
  mintAddress,
  schedule,
  signTransactionInstructions,
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  generateRandomSeed,
  sleep,
  destinationPubkey,
  destinationAccount,
  newDestinationTokenAccountOwner,
} from './utils';
import { ContractInfo, Schedule, VestingScheduleHeader } from './state';
import { assert } from 'console';
import bs58 from 'bs58';
import * as crypto from "crypto"; 

export async function createPool(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  targetPoolTokenKey: PublicKey,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  signalProviderKey: PublicKey,
  depositAmounts: Array<number>,
  payer: PublicKey,
): Promise<Array<TransactionInstruction>> {

  let pool_seed: Uint8Array;
  let poolKey: PublicKey;
  let bump: number;
  // Find a valid pool seed
  while (true) {
    pool_seed = crypto.randomBytes(32);
    [poolKey, bump] = await PublicKey.findProgramAddress(
      [pool_seed],
      bonfidaBotProgramId,
    );
    pool_seed[31] = bump;
    try {
      await PublicKey.createProgramAddress([pool_seed, new Uint8Array(1)], bonfidaBotProgramId);
      break;
    } catch (e) {
      continue;
    }
  }
  let poolMintKey = PublicKey.createProgramAddress([pool_seed, new Uint8Array(1)], bonfidaBotProgramId);

  const vestingTokenAccountKey = await findAssociatedTokenAddress(
    poolKey,
    mintAddress,
  );

  console.log('contract ID: ', bs58.encode(pool_seed));

  const check_existing = await connection.getAccountInfo(poolKey);
  if (!!check_existing) {
    throw 'Pool already exists.';
  }

  let instruction = [
    createInitInstruction(
      SystemProgram.programId,
      programId,
      payer,
      vestingAccountKey,
      [seedWord],
      schedules.length,
    ),
    await createAssociatedTokenAccount(
      SystemProgram.programId,
      SYSVAR_CLOCK_PUBKEY,
      payer,
      vestingAccountKey,
      mintAddress,
    ),
    createCreateInstruction(
      programId,
      TOKEN_PROGRAM_ID,
      vestingAccountKey,
      vestingTokenAccountKey,
      sourceTokenOwner,
      possibleSourceTokenPubkey,
      destinationTokenPubkey,
      mintAddress,
      schedules,
      [seedWord],
    ),
  ];
  return instruction;
}

const test = async (): Promise<void> => {
  const seed = generateRandomSeed();
  console.log(`Seed ${seed}`);
  const instructions = await createPool(
    connection,
    VESTING_PROGRAM_ID,
    Buffer.from(seed, 'hex'),
    account.publicKey,
    account.publicKey,
    tokenPubkey,
    destinationPubkey,
    mintAddress,
    [schedule],
  );
  const signed = await signTransactionInstructions(
    connection,
    [account],
    account.publicKey,
    instructions,
  );
  console.log('✅ Successfully created vesting instructions');
  console.log(`🚚 Transaction signature: ${signed} \n`);

};

test();
