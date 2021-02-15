import { PublicKey, SYSVAR_RENT_PUBKEY, TransactionInstruction } from '@solana/web3.js';
import { Numberu32, Numberu64 } from './utils';

export enum Instruction {
  Init,
  Create,
}

export function initInstruction(
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
    // @ts-ignore
    new Numberu32(maxNumberOfAssets).toBuffer(),
  ];

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: systemProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: rentProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: mintKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: payerKey,
      isSigner: true,
      isWritable: true,
    },
  ];

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function initOrderInstruction(
  systemProgramId: PublicKey,
  rentProgramId: PublicKey,
  bonfidaBotProgramId: PublicKey,
  orderTrackerKey: PublicKey,
  openOrdersKey: PublicKey,
  payerKey: PublicKey,
  poolKey: PublicKey,
  poolSeed: Array<Buffer | Uint8Array>,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([1]).buffer),
    Buffer.concat(poolSeed),
  ];

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: systemProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: rentProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: orderTrackerKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: openOrdersKey,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: payerKey,
      isSigner: true,
      isWritable: true,
    },
  ];

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function createInstruction(
  splTokenProgramId: PublicKey,
  bonfidaBotProgramId: PublicKey,
  mintKey: PublicKey,
  poolKey: PublicKey,
  poolSeed: Array<Buffer | Uint8Array>,
  poolAssetKeys: Array<PublicKey>,
  targetPoolTokenKey: PublicKey,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  signalProviderKey: PublicKey,
  depositAmounts: Array<number>,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([2]).buffer),
    Buffer.concat(poolSeed),
    signalProviderKey.toBuffer()
  ];
  for (var amount of depositAmounts) {
    // @ts-ignore
    buffers.push(new Numberu64(amount).toBuffer())
  }

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: mintKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: targetPoolTokenKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: true,
    },
  ];
  for (var poolAsset of poolAssetKeys) {
    keys.push({
      pubkey: poolAsset,
      isSigner: false,
      isWritable: true,
    })
  }
  keys.push({
    pubkey: sourceOwnerKey,
    isSigner: true,
    isWritable: false,
  })
  for (var sourceAsset of sourceAssetKeys) {
    keys.push({
      pubkey: sourceAsset,
      isSigner: false,
      isWritable: true,
    })
  }

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function depositInstruction(
  splTokenProgramId: PublicKey,
  bonfidaBotProgramId: PublicKey,
  mintKey: PublicKey,
  poolKey: PublicKey,
  poolAssetKeys: Array<PublicKey>,
  targetPoolTokenKey: PublicKey,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  poolSeed: Array<Buffer | Uint8Array>,
  poolTokenAmount: number,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([3]).buffer),
    Buffer.concat(poolSeed),
    // @ts-ignore
    new Numberu64(poolTokenAmount).toBuffer()
  ];

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: mintKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: targetPoolTokenKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: false,
    },
  ];
  for (var poolAsset of poolAssetKeys) {
    keys.push({
      pubkey: poolAsset,
      isSigner: false,
      isWritable: true,
    })
  }
  keys.push({
    pubkey: sourceOwnerKey,
    isSigner: true,
    isWritable: false,
  })
  for (var sourceAsset of sourceAssetKeys) {
    keys.push({
      pubkey: sourceAsset,
      isSigner: false,
      isWritable: true,
    })
  }

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}