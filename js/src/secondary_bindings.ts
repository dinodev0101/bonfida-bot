import {
  Account,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
  Connection,
  TokenAmount,
} from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, AccountLayout, u64 } from '@solana/spl-token';
import { Market, TOKEN_MINTS, MARKETS } from '@project-serum/serum';
import { depositInstruction } from './instructions';
import {
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  Numberu64,
  getMarketData,
  getMidPrice,
  signAndSendTransactionInstructions,
  sleep,
  findAndCreateAssociatedAccount,
} from './utils';
import {
  PoolHeader,
  PoolStatus,
  PUBKEY_LENGTH,
  unpack_assets,
  unpack_markets,
} from './state';
import { PoolAssetBalance } from './types';
import { BONFIDABOT_PROGRAM_ID, BONFIDA_BNB_KEY, BONFIDA_FEE_KEY, createPool, SERUM_PROGRAM_ID } from './main';
import { connect } from 'http2';
import Wallet from '@project-serum/sol-wallet-adapter';

export type PoolInfo = {
  address: PublicKey;
  serumProgramId: PublicKey;
  seed: Uint8Array;
  signalProvider: PublicKey;
  status: PoolStatus;
  mintKey: PublicKey;
  assetMintkeys: Array<PublicKey>;
  authorizedMarkets: Array<PublicKey>;
};

export async function fetchPoolInfo(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  poolSeed: Buffer | Uint8Array,
): Promise<PoolInfo> {
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    bonfidaBotProgramId,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    bonfidaBotProgramId,
  );
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolData.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolData.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let authorizedMarkets = unpack_markets(
    poolData.data.slice(
      PoolHeader.LEN,
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
    poolHeader.numberOfMarkets,
  );

  let poolInfo: PoolInfo = {
    address: poolKey,
    serumProgramId: poolHeader.serumProgramId,
    seed: poolHeader.seed,
    signalProvider: poolHeader.signalProvider,
    status: poolHeader.status,
    mintKey: poolMintKey,
    assetMintkeys: poolAssets.map(asset => asset.mintAddress),
    authorizedMarkets,
  };

  return poolInfo;
}

// Fetch the balances of the poolToken and the assets (in the same order as in the poolData)
export async function fetchPoolBalances(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  poolSeed: Buffer | Uint8Array,
): Promise<[TokenAmount, Array<PoolAssetBalance>]> {
  let poolKey = await PublicKey.createProgramAddress(
    [poolSeed],
    bonfidaBotProgramId,
  );
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    bonfidaBotProgramId,
  );
  let poolData = await connection.getAccountInfo(poolKey);
  if (!poolData) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolData.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolData.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  let assetBalances: Array<PoolAssetBalance> = [];
  for (let asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    let balance = (await connection.getTokenAccountBalance(assetKey)).value;
    assetBalances.push({
      tokenAmount: balance,
      mint: asset.mintAddress.toBase58(),
    });
  }

  let poolTokenSupply = (await connection.getTokenSupply(poolMintKey)).value;

  return [poolTokenSupply, assetBalances];
}
 

// This method lets the user deposit an arbitrary token into the pool
// by intermediately trading with serum in order to reach the pool asset ratio
export async function singleTokenDeposit(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourceOwner: Wallet,
  sourceTokenKey: PublicKey,
  // The amount of source tokens to invest into pool
  amount: number,
  poolSeed: Buffer | Uint8Array,
  payer: Account,
) {
  // Fetch Poolasset mints
  console.log("Creating source asset accounts");
  let poolKey = await PublicKey.createProgramAddress([poolSeed], bonfidaBotProgramId);
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    bonfidaBotProgramId,
  );
  let poolInfo = await connection.getAccountInfo(poolKey);
  if (!poolInfo) {
    throw 'Pool account is unavailable';
  }
  let poolHeader = PoolHeader.fromBuffer(
    poolInfo.data.slice(0, PoolHeader.LEN),
  );
  let poolAssets = unpack_assets(
    poolInfo.data.slice(
      PoolHeader.LEN + Number(poolHeader.numberOfMarkets) * PUBKEY_LENGTH,
    ),
  );

  // Transfer source tokens to USDC
  let tokenInfo = await connection.getAccountInfo(sourceTokenKey);
  if (!tokenInfo) {
    throw 'Source asset account is unavailable';
  }
  let tokenData = Buffer.from(tokenInfo.data);
  const tokenMint = new PublicKey(AccountLayout.decode(tokenData).mint);
  const tokenInitialBalance: number = AccountLayout.decode(tokenData).amount;
  let tokenSymbol = TOKEN_MINTS[
    TOKEN_MINTS.map(t => t.address.toString()).indexOf(tokenMint.toString())
  ].name;

  let midPriceUSDC: number, sourceUSDCKey: PublicKey;
  if (tokenSymbol != "USDC") {
    let pairSymbol = tokenSymbol.concat("/USDC");
    let usdcMarketInfo =
      MARKETS[
        MARKETS.map(m => {
          return m.name;
        }).lastIndexOf(pairSymbol)
      ];
    if (usdcMarketInfo.deprecated) {
      throw 'Chosen Market is deprecated';
    }
    
    let marketUSDC: Market;
    [marketUSDC, midPriceUSDC] = await getMidPrice(connection, usdcMarketInfo.address);

    console.log(tokenInitialBalance);
    console.log("Creating token to USDC order");
    console.log({
      owner: sourceOwner.publicKey.toString(),
      payer: sourceTokenKey.toString(),
      side: 'sell',
      price: 0.95 * midPriceUSDC,
      size: amount,
      orderType: 'ioc',
    });
    await marketUSDC.placeOrder(connection, {
      owner: sourceOwner,
      payer: sourceTokenKey,
      side: 'sell',
      price: 0.95 * midPriceUSDC,
      size: amount,
      orderType: 'ioc',
    });

    sourceUSDCKey = await findAssociatedTokenAddress(
      sourceOwner.publicKey,
      marketUSDC.quoteMintAddress
    );
    let sourceUSDCInfo = await connection.getAccountInfo(sourceUSDCKey);
    if (!sourceUSDCInfo) {
      let createUSDCInstruction = await createAssociatedTokenAccount(
        SystemProgram.programId,
        payer.publicKey,
        sourceOwner.publicKey,
        marketUSDC.quoteMintAddress
      );
      await signAndSendTransactionInstructions(
        connection,
        [],
        payer,
        [createUSDCInstruction],
      );
    }

    // TODO potentially wait for match
    await sleep(30 * 1000);

    // Settle the sourceToken to USDC transfer
    console.log("Settling order");
    let openOrders = await marketUSDC.findOpenOrdersAccountsForOwner(
      connection,
      sourceOwner.publicKey,
    );
    for (let openOrder of openOrders) {
      await marketUSDC.settleFunds(
        connection,
        sourceOwner,
        openOrder,
        sourceTokenKey,
        sourceUSDCKey
      );
    }
  } else {
    midPriceUSDC = 1;
    sourceUSDCKey = sourceTokenKey;
  }

  // Verify that order went through correctly
  tokenInfo = await connection.getAccountInfo(sourceTokenKey);
  if (!tokenInfo) {
    throw 'Source asset account is unavailable';
  }
  tokenData = Buffer.from(tokenInfo.data);
  let tokenBalance = AccountLayout.decode(tokenData).amount;
  if (tokenInitialBalance - tokenBalance > amount) {
    throw 'Conversion to USDC Order was not matched.';
  }

  // Create the source asset account if nonexistent
  let createAssetInstructions: TransactionInstruction[] = new Array();
  let sourceAssetKeys: Array<PublicKey> = [];
  let poolAssetKeys: Array<PublicKey> = [];
  for (let asset of poolAssets) {
    let sourceAssetKey = await findAssociatedTokenAddress(
      sourceOwner.publicKey,
      asset.mintAddress,
    );
    sourceAssetKeys.push(sourceAssetKey);
    let poolAssetKey = await findAssociatedTokenAddress(
      poolKey,
      asset.mintAddress,
    );
    poolAssetKeys.push(poolAssetKey);
    let sourceAssetInfo = await connection.getAccountInfo(sourceAssetKey);
    if (!sourceAssetInfo) {
      createAssetInstructions.push(
        await createAssociatedTokenAccount(
          SystemProgram.programId,
          payer.publicKey,
          sourceOwner.publicKey,
          asset.mintAddress,
        ),
      );
    }
  }
  if (createAssetInstructions.length > 0) {
    await signAndSendTransactionInstructions(
      connection,
      [],
      payer,
      createAssetInstructions,
    );
  }


  // Buy the corresponding tokens with the source USDC in correct ratios 
  console.log("Invest USDC in pool ratios");
  let totalPoolAssetAmount: number = 0;
  let poolAssetAmounts: Array<number> = [];
  for (let asset of poolAssets) {
    let poolAssetKey = await findAssociatedTokenAddress(
      poolKey,
      asset.mintAddress,
    );
    let poolAssetBalance = +(
      await connection.getTokenAccountBalance(poolAssetKey)
    ).value.amount;
    poolAssetAmounts.push(poolAssetBalance);
    totalPoolAssetAmount += poolAssetBalance;
  }
  let poolAssetMarkets: Array<Market | undefined> = [];
  let poolTokenAmount = 0;
  for (let i = 0; i < poolAssets.length; i++) {
    let poolAssetSymbol =
      TOKEN_MINTS[
        TOKEN_MINTS.map(t => t.address.toString()).indexOf(
          poolAssets[i].mintAddress.toString(),
        )
      ].name;
    if (poolAssetSymbol != "USDC") {
      let assetPairSymbol = poolAssetSymbol.concat('/USDC');

      let assetMarketInfo =
        MARKETS[
          MARKETS.map(m => {
            return m.name;
          }).lastIndexOf(assetPairSymbol)
        ];
      if (assetMarketInfo.deprecated) {
        throw 'Chosen Market is deprecated';
      }

      let [assetMarket, assetMidPrice] = await getMidPrice(
        connection,
        assetMarketInfo.address,
      );
      poolAssetMarkets.push(assetMarket);
      let assetAmountToBuy =
        (midPriceUSDC * amount * poolAssetAmounts[i]) /
        (assetMidPrice * totalPoolAssetAmount);
      poolTokenAmount = Math.max(
        poolTokenAmount,
        assetAmountToBuy / poolAssetAmounts[i],
      );
      console.log(assetPairSymbol);
      console.log({
        owner: sourceOwner.publicKey.toString(),
        payer: sourceUSDCKey.toString(),
        side: 'buy',
        price: 1.05 * assetMidPrice,
        size: assetAmountToBuy,
        orderType: 'ioc',
      });
      assetMarket.placeOrder(connection, {
        owner: sourceOwner,
        payer: sourceUSDCKey,
        side: 'buy',
        price: 1.05 * assetMidPrice,
        size: assetAmountToBuy,
        orderType: 'ioc',
      });
    } else {
      poolAssetMarkets.push(undefined);
      poolTokenAmount = Math.max(
        poolTokenAmount,
        1000000 * midPriceUSDC * amount / totalPoolAssetAmount,
      );
    }
  }

  // TODO potentially wait for match
  await sleep(5 * 1000);

  // Settle the USDC to Poolassets transfers
  console.log("Settling the orders");
  for (let i=0; i < poolAssets.length; i++) {
    let assetMarket = poolAssetMarkets[i]
    if (!!assetMarket) {
      let openOrders = await assetMarket.findOpenOrdersAccountsForOwner(
        connection,
        sourceOwner.publicKey,
      );
      for (let openOrder of openOrders) {
        await assetMarket.settleFunds(
          connection,
          sourceOwner,
          openOrder,
          sourceAssetKeys[i],
          sourceUSDCKey,
        );
      }
    }
  }

  // If nonexistent, create the source owner and signal provider associated addresses to receive the pooltokens
  let instructions: Array<TransactionInstruction> = [];
  let [targetPoolTokenKey, targetPTInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    sourceOwner.publicKey,
    poolMintKey,
    payer.publicKey
  );
  targetPTInstruction? instructions.push(targetPTInstruction) : null;

  let [sigProviderFeeReceiverKey, sigProvInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    poolHeader.signalProvider,
    poolMintKey,
    payer.publicKey
  );
  sigProvInstruction? instructions.push(sigProvInstruction) : null;

  let [bonfidaFeeReceiverKey, bonfidaFeeInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    BONFIDA_FEE_KEY,
    poolMintKey,
    payer.publicKey
  );
  bonfidaFeeInstruction? instructions.push(bonfidaFeeInstruction) : null;

  let [bonfidaBuyAndBurnKey, bonfidaBNBInstruction] = await findAndCreateAssociatedAccount(
    SystemProgram.programId,
    connection,
    BONFIDA_BNB_KEY,
    poolMintKey,
    payer.publicKey
  );
  bonfidaBNBInstruction? instructions.push(bonfidaBNBInstruction) : null;


  // @ts-ignore
  console.log(poolTokenAmount, new Numberu64(1000000 * poolTokenAmount));

  // Do the effective deposit
  console.log("Execute Buy in");
  let depositTxInstruction = depositInstruction(
    TOKEN_PROGRAM_ID,
    bonfidaBotProgramId,
    sigProviderFeeReceiverKey,
    bonfidaFeeReceiverKey,
    bonfidaBuyAndBurnKey,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwner.publicKey,
    sourceAssetKeys,
    [poolSeed],
    // @ts-ignore
    new Numberu64(1000000 * poolTokenAmount),
  );
  instructions.push(depositTxInstruction);
  console.log(await signAndSendTransactionInstructions(
    connection,
    [sourceOwner],
    payer,
    instructions,
  ));
}

// Get the seeds of the pools managed by the given signal provider
// Gets all poolseeds if no signal provider was given
export async function getPoolsSeedsBySigProvider(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  signalProviderKey?: PublicKey,
): Promise<Buffer[]> {
  const filter = [];
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
    let data = Buffer.from(account['account']['data'][0], 'base64');
    if (data.length < PoolHeader.LEN) {
      continue;
    }
    if (
      !signalProviderKey ||
      new PublicKey(data.slice(64, 96)).equals(signalProviderKey)
    ) {
      poolSeeds.push(data.slice(32, 64));
    }
  }
  return poolSeeds;
}

// TODO 2nd layer bindings: settle all(find open orders by owner) + settle&redeem + cancelall

// Returns the pool token mint given a pool seed
export const getPoolTokenMintFromSeed = async (
  poolSeed: Buffer | Uint8Array,
  bonfidaBotProgramId: PublicKey,
) => {
  let array_one = new Uint8Array(1);
  array_one[0] = 1;
  let poolMintKey = await PublicKey.createProgramAddress(
    [poolSeed, array_one],
    bonfidaBotProgramId,
  );
  return poolMintKey;
}
