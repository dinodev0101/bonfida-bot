import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  TransactionInstruction,
  Connection,
  CreateAccountParams,
  TokenAmount,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, AccountLayout } from '@solana/spl-token';
import { Market, TOKEN_MINTS, MARKETS } from "@project-serum/serum";
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
  getMidPrice,
  ASSOCIATED_TOKEN_PROGRAM_ID
} from './utils';
import { OrderSide, OrderType, PoolAsset, PoolHeader, PoolStatus, PUBKEY_LENGTH, SelfTradeBehavior, unpack_assets, unpack_markets } from './state';
import bs58 from 'bs58';
import * as crypto from "crypto";

export type PoolInfo = {
  address: PublicKey,
  serumProgramId: PublicKey,
  seed: Uint8Array,
  signalProvider: PublicKey,
  status: PoolStatus,
  mintKey: PublicKey,
  assetMintkeys: Array<PublicKey>,
  authorizedMarkets: Array<PublicKey>
};

export async function fetchPoolInfo(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  poolSeed: Buffer | Uint8Array,
): Promise<PoolInfo> {
  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress([poolSeed, array_one], bonfidaBotProgramId);
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(poolData.data.slice(0, PoolHeader.LEN));
  let poolAssets = unpack_assets(poolData.data.slice(
    PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
  ));

  let authorizedMarkets = unpack_markets(poolData.data.slice(
    PoolHeader.LEN, PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
  ), poolHeader.numberOfMarkets);

  let poolInfo: PoolInfo = {
    address: poolKey,
    serumProgramId: poolHeader.serumProgramId,
    seed: poolHeader.seed,
    signalProvider: poolHeader.signalProvider,
    status: poolHeader.status,
    mintKey: poolMintKey,
    assetMintkeys: poolAssets.map(asset => asset.mintAddress),
    authorizedMarkets
  };

  return poolInfo
}

// Fetch the balances of the poolToken and the assets (in the same order as in the poolData)
export async function fetchPoolBalances(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  poolSeed: Buffer | Uint8Array,
): Promise<[TokenAmount, Array<TokenAmount>]> {

  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress([poolSeed, array_one], bonfidaBotProgramId);
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(poolData.data.slice(0, PoolHeader.LEN));
  let poolAssets = unpack_assets(poolData.data.slice(
    PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
  ));

  let authorizedMarkets = unpack_markets(poolData.data.slice(
    PoolHeader.LEN, PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
  ), poolHeader.numberOfMarkets);
  
  let assetBalances: Array<TokenAmount> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(
      poolKey,
      asset.mintAddress
    );
    let balance = (await connection.getTokenAccountBalance(assetKey)).value;
    assetBalances.push(balance);
  }

  let poolTokenSupply = (await connection.getTokenSupply(poolMintKey)).value;

  return [poolTokenSupply, assetBalances]
}

// This method lets the user deposit an arbitrary token into the pool
// by intermediately trading with serum in order to reach the pool asset ratio
// export async function singleTokenDeposit(
//   connection: Connection,
//   bonfidaBotProgramId: PublicKey,
//   sourceOwner: Account,
//   sourceTokenKey: PublicKey,
//   // The amount of source tokens to invest into pool
//   amount: number,
//   poolSeed: Buffer | Uint8Array,
//   payerKey: PublicKey,
// ): Promise<TransactionInstruction[]> {
  
//   // Transfer source tokens to USDC
//   let tokenInfo = await connection.getAccountInfo(sourceTokenKey);
//   if (!tokenInfo) {
//     throw 'Source asset account is unavailable';
//   }
//   let tokenData = Buffer.from(tokenInfo.data);
//   const tokenMint = new PublicKey(AccountLayout.decode(tokenData).mint);
//   let tokenSymbol = TOKEN_MINTS[
//     TOKEN_MINTS.map(t => t.address.toString()).indexOf(tokenMint.toString())
//   ].name;
//   let pairSymbol = tokenSymbol.concat("USDC");

//   let marketInfo = MARKETS[MARKETS.map(m => {return m.name}).lastIndexOf(pairSymbol)];
//   if (marketInfo.deprecated) {throw "Chosen Market is deprecated"};
//   let marketData = await getMarketData(connection, marketInfo.address);

//   let [market, midPrice] = await getMidPrice(marketInfo.address);
//   await market.placeOrder(connection, {
//     owner: sourceOwner,
//     payer: payerKey,
//     side: 'sell',
//     price: midPrice,
//     size: amount,
//     orderType: 'limit',
//   });

//   let openOrders = await market.findOpenOrdersAccountsForOwner(
//     connection,
//     sourceOwner.publicKey
//   );
//   openOrders


//   await market.settleFunds(
//     connection,
//     sourceOwner,
    
//   )

   

//   // Fetch Poolasset mints
//   let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);
//   let array_one = new Uint8Array(1);
//   array_one[0] = 1;
//   let poolMintKey = await PublicKey.createProgramAddress([poolSeed, array_one], bonfidaBotProgramId);
//   let poolInfo = await connection.getAccountInfo(poolKey);
//   if (!poolInfo) {
//     throw 'Pool account is unavailable';
//   }
//   let poolHeader = PoolHeader.fromBuffer(poolInfo.data.slice(0, PoolHeader.LEN));
//   let poolAssets = unpack_assets(poolInfo.data.slice(
//     PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH
//   ));

//   for (var asset of poolAssets) {
//     asset.mintAddress = 
//   }
  
//   let marketData = await getMarketData(connection, market);
// }

// Get the seeds of the pools managed by the given signal provider
// Gets all poolseeds if no signal provider was given
export async function getPoolsSeedsBySigProvider(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  signalProviderKey: PublicKey | undefined,
): Promise<Buffer[]> {

  const filter = []
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
    if ((!signalProviderKey) || ((new PublicKey(data.slice(64, 96))).equals(signalProviderKey))) {
      poolSeeds.push(data.slice(32, 64));
    }
  }
  return poolSeeds
}

// TODO 2nd layer bindings: iterative deposit + settle all(find open orders by owner) + settle&redeem + cancelall + create_easy 
// TODO adapt bindings to Elliott push in state