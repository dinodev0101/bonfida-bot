import { PublicKey } from '@solana/web3.js';
import { Numberu64 } from './utils';

const STATUS_PENDING_ORDER_FLAG: number = 1 << 6;
const STATUS_PENDING_ORDER_MASK: number = 0x3f;
const STATUS_LOCKED_FLAG: number = 2 << 6;
const STATUS_UNLOCKED_FLAG: number = STATUS_PENDING_ORDER_MASK;

export enum PoolStatusID {
  Uninitialized,
  Unlocked,
  Locked,
  PendingOrder,
  LockedPendingOrder
}

export type PoolStatus = [PoolStatusID, number];

export class PoolHeader {
  static LEN = 33;
  signalProvider!: PublicKey;
  status!: PoolStatus;

  constructor(signalProvider: PublicKey, status: PoolStatus) {
    this.signalProvider = signalProvider;
    this.status = status;
  }

  // function match_status(status_byte): 

  // public toBuffer(): Buffer {
  //   let result = [this.signalProvider.toBuffer()];
  //   let status_byte = 0x2;
  //   switch(status_byte) {
  //     case PoolStatus.Uninitialized:
  //       0
  //     case 
  //   }
  // }

  static match_status(status_byte: Buffer): PoolStatus {
    let sByte = status_byte.readInt8();
    switch(sByte >> 6) {
      case 0:
        return [PoolStatusID.Unlocked, 0]
      case 1:
        return [
          PoolStatusID.PendingOrder,
          (sByte & STATUS_PENDING_ORDER_MASK) + 1
        ]
      case 2: 
        return [PoolStatusID.Locked, 0]
      case 3: 
        return [
          PoolStatusID.LockedPendingOrder,
          (sByte & STATUS_PENDING_ORDER_MASK) + 1
        ]
      default:
        throw "Pool status byte could not be parsed."
    }
  }

  static fromBuffer(buf: Buffer): PoolHeader {
    const signalProvider: PublicKey = new PublicKey(buf.slice(0, 32));
    const status: PoolStatus = PoolHeader.match_status(buf.slice(32, 33));
    return new PoolHeader(signalProvider, status);
  }
}

export class PoolAsset {
  static LEN = 40;
  // Release time in unix timestamp
  mintAddress!: PublicKey;
  amountInToken!: Numberu64;

  constructor(mintAddress: PublicKey, amountInToken: Numberu64) {
    this.mintAddress = mintAddress;
    this.amountInToken = amountInToken;
  }

  public toBuffer(): Buffer {
    return Buffer.concat([
      this.mintAddress.toBuffer(),
      this.amountInToken.toBuffer(),
    ]);
  }

  static fromBuffer(buf: Buffer): PoolAsset {
    const mintAddress: PublicKey = new PublicKey(buf.slice(0, 32));
    const amountInToken: Numberu64 = Numberu64.fromBuffer(buf.slice(32, 40));
    return new PoolAsset(mintAddress, amountInToken);
  }
}

export function unpack_assets(input: Buffer): Array<PoolAsset> {
  let numberOfAssets = input.length / PoolAsset.LEN;
  let output: Array<PoolAsset> = [];
  let offset = 0;
  let zeroArray: Int8Array = new Int8Array(32);
  zeroArray.fill(0);
  for (let i=0; i<numberOfAssets; i++) {
    let asset = PoolAsset.fromBuffer(input.slice(offset, offset + PoolAsset.LEN));
    if (asset.mintAddress != new PublicKey(Buffer.from(zeroArray))) {
      output.push(asset);
    }
    offset += PoolAsset.LEN;
  }
  return output;
}

// export class ContractInfo {
//   destinationAddress!: PublicKey;
//   mintAddress!: PublicKey;
//   schedules!: Array<Schedule>;

//   constructor(
//     destinationAddress: PublicKey,
//     mintAddress: PublicKey,
//     schedules: Array<Schedule>,
//   ) {
//     this.destinationAddress = destinationAddress;
//     this.mintAddress = mintAddress;
//     this.schedules = schedules;
//   }

//   static fromBuffer(buf: Buffer): ContractInfo | undefined {
//     const header = VestingScheduleHeader.fromBuffer(buf.slice(0, 65));
//     if (!header.isInitialized) {
//       return undefined;
//     }
//     const schedules: Array<Schedule> = [];
//     for (let i = 65; i < buf.length; i += 16) {
//       schedules.push(Schedule.fromBuffer(buf.slice(i, i + 16)));
//     }
//     return new ContractInfo(
//       header.destinationAddress,
//       header.mintAddress,
//       schedules,
//     );
//   }
// }
