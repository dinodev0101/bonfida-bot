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
  createInitInstruction,
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

export async function createPool(
  connection: Connection,
  programId: PublicKey,
  seedWord: Buffer | Uint8Array,
  payer: PublicKey,
  sourceTokenOwner: PublicKey,
  possibleSourceTokenPubkey: PublicKey | null,
  destinationTokenPubkey: PublicKey,
  mintAddress: PublicKey,
  schedules: Array<Schedule>,
): Promise<Array<TransactionInstruction>> {
  // If no source token account was given, use the associated source account
  if (possibleSourceTokenPubkey == null) {
    possibleSourceTokenPubkey = await findAssociatedTokenAddress(
      sourceTokenOwner,
      mintAddress,
    );
  }

  // Find the non reversible public key for the vesting contract via the seed
  seedWord = seedWord.slice(0, 31);
  const [vestingAccountKey, bump] = await PublicKey.findProgramAddress(
    [seedWord],
    programId,
  );

  const vestingTokenAccountKey = await findAssociatedTokenAddress(
    vestingAccountKey,
    mintAddress,
  );

  seedWord = Buffer.from(seedWord.toString('hex') + bump.toString(16), 'hex');

  console.log(
    'Vesting contract account pubkey: ',
    vestingAccountKey.toBase58(),
  );

  console.log('contract ID: ', bs58.encode(seedWord));

  const check_existing = await connection.getAccountInfo(vestingAccountKey);
  if (!!check_existing) {
    throw 'Contract already exists.';
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
