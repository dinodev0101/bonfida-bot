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


export async function iterativeDeposit(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourceOwnerKey: Account,
  sourceAssetKeys: Array<PublicKey>,
  poolTokenAmount: Numberu64,
  poolSeed: Array<Buffer | Uint8Array>,
  payer: Account,
): Promise<TransactionInstruction[]> {


}

// TODO 2nd layer bindings: iterative deposit + settle all(find open orders by owner) + settle&redeem + cancelall + create_easy  
// TODO Check out coin/pc vs source/target in program instructions