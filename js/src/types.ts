import { Token } from '@solana/spl-token';
import { PublicKey, TokenAmount } from '@solana/web3.js';
import { OrderSide, OrderType, SelfTradeBehavior } from './state';
import { Numberu16, Numberu64 } from './utils';

export interface PoolAssetBalance {
  tokenAmount: TokenAmount;
  mint: string;
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
    transactionSlot: number
}
