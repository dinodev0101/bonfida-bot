import { PublicKey, SYSVAR_RENT_PUBKEY, TransactionInstruction } from '@solana/web3.js';
import { Schedule } from './state';
import { Numberu32 } from './utils';

export enum Instruction {
  Init,
  Create,
}

export function InitInstruction(
  splTokenProgramId: PublicKey,
  systemProgramId: PublicKey,
  rentProgramId: PublicKey,
  bonfidaBotProgramId: PublicKey,
  mintKey: PublicKey,
  payerKey: PublicKey,
  poolKey: PublicKey,
  poolSeed: Array<Buffer | Uint8Array>,
  maxNumberOfAssets: number,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([0]).buffer),
    Buffer.concat(poolSeed),
    // @ts-ignore TODO
    new Numberu32(numberOfSchedules).toBuffer(),
  ];

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: systemProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: SYSVAR_RENT_PUBKEY,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: payerKey,
      isSigner: true,
      isWritable: true,
    },
    {
      pubkey: vestingAccountKey,
      isSigner: false,
      isWritable: true,
    },
  ];

  return new TransactionInstruction({
    keys,
    programId: vestingProgramId,
    data,
  });
}