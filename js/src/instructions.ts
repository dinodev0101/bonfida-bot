import { PublicKey, TransactionInstruction } from '@solana/web3.js';
import { OrderSide, OrderType, SelfTradeBehavior } from './state';
import { Numberu128, Numberu16, Numberu32, Numberu64 } from './utils';

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
  number_of_markets: Numberu16,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([0])),
    Buffer.concat(poolSeed),
    // @ts-ignore
    new Numberu32(maxNumberOfAssets).toBuffer(),
    number_of_markets.toBuffer(),
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

export function createInstruction(
  splTokenProgramId: PublicKey,
  bonfidaBotProgramId: PublicKey,
  clockSysvarKey: PublicKey,
  mintKey: PublicKey,
  poolKey: PublicKey,
  poolSeed: Array<Buffer | Uint8Array>,
  poolAssetKeys: Array<PublicKey>,
  targetPoolTokenKey: PublicKey,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  serumProgramId: PublicKey,
  signalProviderKey: PublicKey,
  depositAmounts: Array<number>,
  markets: Array<PublicKey>,
  feeCollectionPeriod: Numberu64,
  feeRatio: Numberu64
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([1])),
    Buffer.concat(poolSeed),
    serumProgramId.toBuffer(),
    signalProviderKey.toBuffer(),
    // @ts-ignore
    new Numberu16(markets.length).toBuffer(),
    feeCollectionPeriod.toBuffer(),
    feeRatio.toBuffer()
  ];
  for (var market of markets) {
    // @ts-ignore
    buffers.push(market.toBuffer());
  }
  for (var amount of depositAmounts) {
    // @ts-ignore
    buffers.push(new Numberu64(amount).toBuffer());
  }

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: clockSysvarKey,
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
    });
  }
  keys.push({
    pubkey: sourceOwnerKey,
    isSigner: true,
    isWritable: false,
  });
  for (var sourceAsset of sourceAssetKeys) {
    keys.push({
      pubkey: sourceAsset,
      isSigner: false,
      isWritable: true,
    });
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
  sigProviderFeeReceiverKey: PublicKey,
  bonfidaFeeReceiverKey: PublicKey,
  bonfidaBuyAndBurnKey: PublicKey,
  mintKey: PublicKey,
  poolKey: PublicKey,
  poolAssetKeys: Array<PublicKey>,
  targetPoolTokenKey: PublicKey,
  sourceOwnerKey: PublicKey,
  sourceAssetKeys: Array<PublicKey>,
  poolSeed: Array<Buffer | Uint8Array>,
  poolTokenAmount: Numberu64,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([2])),
    Buffer.concat(poolSeed),
    // @ts-ignore
    poolTokenAmount.toBuffer(),
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
      pubkey: sigProviderFeeReceiverKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: bonfidaFeeReceiverKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: bonfidaBuyAndBurnKey,
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
    });
  }
  keys.push({
    pubkey: sourceOwnerKey,
    isSigner: true,
    isWritable: false,
  });
  for (var sourceAsset of sourceAssetKeys) {
    keys.push({
      pubkey: sourceAsset,
      isSigner: false,
      isWritable: true,
    });
  }

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function createOrderInstruction(
  bonfidaBotProgramId: PublicKey,
  signalProviderKey: PublicKey,
  market: PublicKey,
  payerPoolAssetKey: PublicKey,
  payerPoolAssetIndex: Numberu64,
  targetPoolAssetIndex: Numberu64,
  openOrdersKey: PublicKey,
  serumRequestQueue: PublicKey,
  poolKey: PublicKey,
  coinVaultKey: PublicKey,
  pcVaultKey: PublicKey,
  splTokenProgramId: PublicKey,
  dexProgramKey: PublicKey,
  rentProgramId: PublicKey,
  srmReferrerKey: PublicKey | null,
  poolSeed: Array<Buffer | Uint8Array>,
  side: OrderSide,
  limitPrice: Numberu64,
  market_index: Numberu16,
  coin_lot_size: Numberu64,
  pc_lot_size: Numberu64,
  target_mint: PublicKey,
  maxQuantity: Numberu16,
  orderType: OrderType,
  clientId: Numberu64,
  selfTradeBehavior: SelfTradeBehavior,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([3])),
    Buffer.concat(poolSeed),
    Buffer.from(Int8Array.from([side])),
    limitPrice.toBuffer(),
    maxQuantity.toBuffer(),
    Buffer.from(Int8Array.from([orderType])),
    clientId.toBuffer(),
    Buffer.from(Int8Array.from([selfTradeBehavior])),
    payerPoolAssetIndex.toBuffer(),
    targetPoolAssetIndex.toBuffer(),
    market_index.toBuffer(),
    coin_lot_size.toBuffer(),
    pc_lot_size.toBuffer(),
    target_mint.toBuffer(),
  ];
  const data = Buffer.concat(buffers);

  const keys = [
    {
      pubkey: signalProviderKey,
      isSigner: true,
      isWritable: false,
    },
    {
      pubkey: market,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: payerPoolAssetKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: openOrdersKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: serumRequestQueue,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: coinVaultKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: pcVaultKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: rentProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: dexProgramKey,
      isSigner: false,
      isWritable: false,
    },
  ];
  if (!!srmReferrerKey) {
    keys.push({
      pubkey: srmReferrerKey,
      isSigner: false,
      isWritable: true,
    });
  }

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function cancelOrderInstruction(
  bonfidaBotProgramId: PublicKey,
  signalProviderKey: PublicKey,
  market: PublicKey,
  openOrdersKey: PublicKey,
  requestQueue: PublicKey,
  poolKey: PublicKey,
  dexProgramKey: PublicKey,
  poolSeed: Array<Buffer | Uint8Array>,
  side: OrderSide,
  orderId: Numberu128,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([4])),
    Buffer.concat(poolSeed),
    Buffer.from(Int8Array.from([side])),
    orderId.toBuffer(),
  ];
  const data = Buffer.concat(buffers);

  const keys = [
    {
      pubkey: signalProviderKey,
      isSigner: true,
      isWritable: false,
    },
    {
      pubkey: market,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: openOrdersKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: requestQueue,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: dexProgramKey,
      isSigner: false,
      isWritable: false,
    },
  ];

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function settleFundsInstruction(
  bonfidaBotProgramId: PublicKey,
  market: PublicKey,
  openOrdersKey: PublicKey,
  poolKey: PublicKey,
  poolMintKey: PublicKey,
  coinVaultKey: PublicKey,
  pcVaultKey: PublicKey,
  coinPoolAssetKey: PublicKey,
  pcPoolAssetKey: PublicKey,
  vaultSignerKey: PublicKey,
  splTokenProgramId: PublicKey,
  dexProgramKey: PublicKey,
  srmReferrerKey: PublicKey | null,
  poolSeed: Array<Buffer | Uint8Array>,
  pcPoolAssetIndex: Numberu64,
  coinPoolAssetIndex: Numberu64,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([5])),
    Buffer.concat(poolSeed),
    pcPoolAssetIndex.toBuffer(),
    coinPoolAssetIndex.toBuffer(),
  ];
  const data = Buffer.concat(buffers);

  const keys = [
    {
      pubkey: market,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: openOrdersKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: poolKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: poolMintKey,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: coinVaultKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: pcVaultKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: coinPoolAssetKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: pcPoolAssetKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: vaultSignerKey,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: dexProgramKey,
      isSigner: false,
      isWritable: false,
    },
  ];
  if (!!srmReferrerKey) {
    keys.push({
      pubkey: srmReferrerKey,
      isSigner: false,
      isWritable: true,
    });
  }

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function redeemInstruction(
  splTokenProgramId: PublicKey,
  bonfidaBotProgramId: PublicKey,
  clockSysvarKey: PublicKey,
  mintKey: PublicKey,
  poolKey: PublicKey,
  poolAssetKeys: Array<PublicKey>,
  sourcePoolTokenOwnerKey: PublicKey,
  sourcePoolTokenKey: PublicKey,
  targetAssetKeys: Array<PublicKey>,
  poolSeed: Array<Buffer | Uint8Array>,
  poolTokenAmount: Numberu64,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([6])),
    Buffer.concat(poolSeed),
    // @ts-ignore
    new Numberu64(poolTokenAmount).toBuffer(),
  ];

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: clockSysvarKey,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: mintKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: sourcePoolTokenOwnerKey,
      isSigner: true,
      isWritable: false,
    },
    {
      pubkey: sourcePoolTokenKey,
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
    });
  }
  for (var targetAsset of targetAssetKeys) {
    keys.push({
      pubkey: targetAsset,
      isSigner: false,
      isWritable: true,
    });
  }

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}

export function collectFeesInstruction(
  splTokenProgramId: PublicKey,
  clockSysvarKey: PublicKey,
  bonfidaBotProgramId: PublicKey,
  poolKey: PublicKey,
  mintKey: PublicKey,
  signalProviderPoolTokenKey: PublicKey,
  bonfidaFeePoolTokenKey: PublicKey,
  bonfidaBnBPTKey: PublicKey,
  poolSeed: Array<Buffer | Uint8Array>,
): TransactionInstruction {
  let buffers = [
    Buffer.from(Int8Array.from([7])),
    Buffer.concat(poolSeed),
  ];

  const data = Buffer.concat(buffers);
  const keys = [
    {
      pubkey: splTokenProgramId,
      isSigner: false,
      isWritable: false,
    },
    {
      pubkey: clockSysvarKey,
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
      pubkey: signalProviderPoolTokenKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: bonfidaFeePoolTokenKey,
      isSigner: false,
      isWritable: true,
    },
    {
      pubkey: bonfidaBnBPTKey,
      isSigner: false,
      isWritable: true,
    },
  ];

  return new TransactionInstruction({
    keys,
    programId: bonfidaBotProgramId,
    data,
  });
}