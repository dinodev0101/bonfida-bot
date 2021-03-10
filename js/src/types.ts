import { Token } from '@solana/spl-token';
import { PublicKey, TokenAmount } from '@solana/web3.js';
import { getPoolOrderInfos } from './secondary_bindings';
import { OrderSide, OrderType, SelfTradeBehavior } from './state';
import { MarketData, Numberu16, Numberu64 } from './utils';
import bs58 from 'bs58';

export interface PoolAssetBalance {
  tokenAmount: TokenAmount;
  mint: string;
}

export interface PoolInstructionInfo {
  type: string,
  info: PoolOrderInfo | PoolSettleInfo
}

export interface PoolOrderInfo {
    poolSeed: Buffer,
    side: OrderSide,
    limitPrice: number,
    ratioOfPoolAssetsToTrade: number,
    orderType: OrderType,
    clientOrderId: number,
    selfTradeBehavior: SelfTradeBehavior,
    market: PublicKey,
    transactionSignature: string,
    transactionSlot: number,
    transferredAmount: number,
    settledAmount: { tokenMint: string, amount: number }[],
    openOrderAccount: PublicKey
}

export interface PoolSettleInfo {
  openOrderAccount: PublicKey,
  transferredAmounts: { tokenMint: string, amount: number}[],
  market: PublicKey
}

export const loggablePoolOrderInfo = (o: PoolOrderInfo) => {
  let prepared = {
    poolSeed: bs58.encode(o.poolSeed),
    side: ['Bid', 'Ask'][o.side],
    limitPrice: o.limitPrice,
    ratioOfPoolAssetsToTrade: `${(o.ratioOfPoolAssetsToTrade * 100/ 65536).toFixed(2)}%`,
    orderType: ['Limit', 'ImmediateOrCancel', 'PostOnly'][o.orderType],
    clientOrderId: o.clientOrderId,
    selfTradeBehavior: ["DecrementTake", "CancelProvide", "AbortTransaction"][o.selfTradeBehavior],
    market: o.market.toBase58(),
    transactionSignature: o.transactionSignature,
    transactionSlot: o.transactionSlot,
    transferredAmount: o.transferredAmount,
    settledAmount: JSON.stringify(o.settledAmount),
    openOrderAccount: o.openOrderAccount.toBase58()
  };
  return prepared
}
