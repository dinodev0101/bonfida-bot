import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  TransactionInstruction,
  Connection,
  CreateAccountParams,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, AccountLayout } from '@solana/spl-token';
import {
  cancelOrderInstruction,
  createInstruction,
  createOrderInstruction,
  depositInstruction,
  initInstruction,
  settleFundsInstruction,
} from './instructions';
import {
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  Numberu64,
  Numberu16,
  getMarketData,
  Numberu128,
} from './utils';
import { OrderSide, OrderType, PoolAsset, PoolHeader, SelfTradeBehavior, unpack_assets } from './state';
import bs58 from 'bs58';
import * as crypto from "crypto";


// export async function iterativeDeposit(
//   connection: Connection,
//   bonfidaBotProgramId: PublicKey,
//   sourceOwnerKey: Account,
//   sourceAssetKeys: Array<PublicKey>,
//   poolTokenAmount: Numberu64,
//   poolSeed: Array<Buffer | Uint8Array>,
//   payer: Account,
// ): Promise<TransactionInstruction[]> {

// }

export async function getPoolsSeedsBySigProvider(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  signalProviderKey: PublicKey,
): Promise<Buffer[]> {
  const filter = [
    // Does not seem to take effect, filtering is therefore done below
    {
      memcmp: {
        offset: 32,
        bytes: signalProviderKey.toBase58(),
      },
    },
    {
      dataSize: 32,
    },
  ];

  // @ts-ignore
  const resp = await connection._rpcRequest('getProgramAccounts', [
    bonfidaBotProgramId.toBase58(),
    {
      commitment: connection.commitment,
      filter,
      encoding: 'base64',
    },
  ]);
  if (resp.error) {
    throw new Error(resp.error.message);
  }
  let poolSeeds: Buffer[] = [];
  for (var account of resp.result) {
    let data = Buffer.from(account["account"]["data"][0], 'base64');
    if (data.length < PoolHeader.LEN) {
      continue;
    }
    if ((new PublicKey(data.slice(64, 96))).equals(signalProviderKey)) {
      poolSeeds.push(data.slice(32, 64));
    }
  }
  return poolSeeds
}

// TODO 2nd layer bindings: iterative deposit + settle all(find open orders by owner) + settle&redeem + cancelall + create_easy + getAllPools + getPoolsbySigProv 
// TODO adapt bindings to Elliott push