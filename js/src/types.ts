import { Token } from '@solana/spl-token';
import { TokenAmount } from '@solana/web3.js';

export interface PoolAssetBalance {
  tokenAmount: TokenAmount;
  mint: string;
}
