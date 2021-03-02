import { PublicKey } from '@solana/web3.js';
import { Numberu16, Numberu64 } from './utils';

// Serum analog types
export enum OrderSide {
  Bid,
  Ask,
}
export enum OrderType {
  Limit,
  ImmediateOrCancel,
  PostOnly,
}
export enum SelfTradeBehavior {
  DecrementTake,
  CancelProvide,
}

export const PUBKEY_LENGTH: number = 32;

const STATUS_PENDING_ORDER_FLAG: number = 1 << 6;
const STATUS_PENDING_ORDER_MASK: number = 0x3f;
const STATUS_LOCKED_FLAG: number = 2 << 6;
const STATUS_UNLOCKED_FLAG: number = STATUS_PENDING_ORDER_MASK;

export enum PoolStatusID {
  Uninitialized,
  Unlocked,
  Locked,
  PendingOrder,
  LockedPendingOrder,
}

export type PoolStatus = [PoolStatusID, number];

export class PoolHeader {
  static LEN = 117;
  serumProgramId!: PublicKey;
  seed!: Uint8Array;
  signalProvider!: PublicKey;
  status!: PoolStatus;
  numberOfMarkets!: Numberu16;
  feeRatio!: Numberu16;
  lastFeeCollectionTimestamp!: Numberu64;
  feeCollectionPeriod!: Numberu64

  constructor(
    serumProgramId: PublicKey,
    seed: Uint8Array,
    signalProvider: PublicKey,
    status: PoolStatus,
    numberOfMarkets: Numberu16,
    feeRatio: Numberu16,
    lastFeeCollectionTimestamp: Numberu64,
    feeCollectionPeriod: Numberu64
  ) {
    this.serumProgramId = serumProgramId;
    this.seed = seed;
    this.signalProvider = signalProvider;
    this.status = status;
    this.numberOfMarkets = numberOfMarkets;
    this.feeRatio = feeRatio;
    this.lastFeeCollectionTimestamp = lastFeeCollectionTimestamp;
    this.feeCollectionPeriod = feeCollectionPeriod;
  }

  static match_status(status_byte: Buffer): PoolStatus {
    let sByte = status_byte.readInt8(0);
    switch (sByte >> 6) {
      case 0:
        return [PoolStatusID.Unlocked, 0];
      case 1:
        return [
          PoolStatusID.PendingOrder,
          (sByte & STATUS_PENDING_ORDER_MASK) + 1,
        ];
      case 2:
        return [PoolStatusID.Locked, 0];
      case 3:
        return [
          PoolStatusID.LockedPendingOrder,
          (sByte & STATUS_PENDING_ORDER_MASK) + 1,
        ];
      default:
        throw 'Pool status byte could not be parsed.';
    }
  }

  static fromBuffer(buf: Buffer): PoolHeader {
    const serumProgramId: PublicKey = new PublicKey(buf.slice(0, 32));
    const seed: Uint8Array = buf.slice(32, 64);
    const signalProvider: PublicKey = new PublicKey(buf.slice(64, 96));
    const status: PoolStatus = PoolHeader.match_status(buf.slice(96, 97));
    // @ts-ignore
    const numberOfMarkets = Numberu16.fromBuffer(buf.slice(97, 99));
    const feeRatio = Numberu16.fromBuffer(buf.slice(99, 101));
    const lastFeeCollectionTimestamp = Numberu64.fromBuffer(buf.slice(101, 109));
    const feeCollectionPeriod = Numberu16.fromBuffer(buf.slice(109, 117));
    return new PoolHeader(
      serumProgramId,
      seed,
      signalProvider,
      status,
      numberOfMarkets,
      feeRatio,
      lastFeeCollectionTimestamp,
      feeCollectionPeriod
    );
  }
}

export class PoolAsset {
  static LEN = 32;
  mintAddress!: PublicKey;

  constructor(mintAddress: PublicKey) {
    this.mintAddress = mintAddress;
  }

  public toBuffer(): Buffer {
    return Buffer.concat([this.mintAddress.toBuffer()]);
  }

  static fromBuffer(buf: Buffer): PoolAsset {
    const mintAddress: PublicKey = new PublicKey(buf.slice(0, 32));
    return new PoolAsset(mintAddress);
  }
}

export function unpack_assets(input: Buffer): Array<PoolAsset> {
  let numberOfAssets = input.length / PoolAsset.LEN;
  let output: Array<PoolAsset> = [];
  let offset = 0;
  let zeroArray: Int8Array = new Int8Array(32);
  zeroArray.fill(0);
  for (let i = 0; i < numberOfAssets; i++) {
    let asset = PoolAsset.fromBuffer(
      input.slice(offset, offset + PoolAsset.LEN),
    );
    if (
      asset.mintAddress.toString() !=
      new PublicKey(Buffer.from(zeroArray)).toString()
    ) {
      output.push(asset);
    }
    offset += PoolAsset.LEN;
  }
  return output;
}

export function unpack_markets(
  input: Buffer,
  numberOfMarkets: Numberu16,
): Array<PublicKey> {
  let markets: Array<PublicKey> = new Array();
  let offset = 0;
  for (var i = 0; i < new Number(numberOfMarkets); i++) {
    markets.push(new PublicKey(input.slice(offset, offset + 32)));
    offset += 32;
  }
  return markets;
}
