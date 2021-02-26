import {
  Account,
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  TransactionInstruction,
  Connection,
  CreateAccountParams,
  TokenAmount,
  InstructionType,
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
} from './utils';
import {
  OrderSide,
  OrderType,
  PoolAsset,
  PoolHeader,
  PoolStatus,
  PUBKEY_LENGTH,
  SelfTradeBehavior,
  unpack_assets,
  unpack_markets,
} from './state';

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
): Promise<[TokenAmount, Array<TokenAmount>]> {
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

  let assetBalances: Array<TokenAmount> = [];
  for (var asset of poolAssets) {
    let assetKey = await findAssociatedTokenAddress(poolKey, asset.mintAddress);
    let balance = (await connection.getTokenAccountBalance(assetKey)).value;
    assetBalances.push(balance);
  }

  let poolTokenSupply = (await connection.getTokenSupply(poolMintKey)).value;

  return [poolTokenSupply, assetBalances];
}

// This method lets the user deposit an arbitrary token into the pool
// by intermediately trading with serum in order to reach the pool asset ratio
export async function singleTokenDeposit(
  connection: Connection,
  bonfidaBotProgramId: PublicKey,
  sourceOwner: Account,
  sourceTokenKey: PublicKey,
  // The amount of source tokens to invest into pool
  amount: number,
  poolSeed: Buffer | Uint8Array,
  payer: Account,
): Promise<string> {
  // Transfer source tokens to USDC
  let tokenInfo = await connection.getAccountInfo(sourceTokenKey);
  if (!tokenInfo) {
    throw 'Source asset account is unavailable';
  }
  let tokenData = Buffer.from(tokenInfo.data);
  const tokenMint = new PublicKey(AccountLayout.decode(tokenData).mint);
  const tokenInitialBalance = AccountLayout.decode(tokenData).amount;
  let tokenSymbol =
    TOKEN_MINTS[
      TOKEN_MINTS.map(t => t.address.toString()).indexOf(tokenMint.toString())
    ].name;
  let pairSymbol = tokenSymbol.concat('/USDC');

  let usdcMarketInfo =
    MARKETS[
      MARKETS.map(m => {
        return m.name;
      }).lastIndexOf(pairSymbol)
    ];
  if (usdcMarketInfo.deprecated) {
    throw 'Chosen Market is deprecated';
  }
  let usdcMarketData = await getMarketData(connection, usdcMarketInfo.address);

  let [marketUSDC, midPriceUSDC] = await getMidPrice(
    connection,
    usdcMarketInfo.address,
  );
  await marketUSDC.placeOrder(connection, {
    owner: sourceOwner,
    payer: payer.publicKey,
    side: 'sell',
    price: midPriceUSDC,
    size: amount,
    orderType: 'ioc',
  });

  // Fetch Poolasset mints
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
  await signAndSendTransactionInstructions(
    connection,
    [],
    payer,
    createAssetInstructions,
  );

  // TODO potentially wait for match
  await sleep(30 * 1000);

  // Settle the sourceToken to USDC transfer
  let openOrders = await marketUSDC.findOpenOrdersAccountsForOwner(
    connection,
    sourceOwner.publicKey,
  );
  let openOrder =
    openOrders[
      openOrders
        .map(o => o.market.toString())
        .indexOf(marketUSDC.address.toString())
    ];
  let sourceUSDCKey = await findAssociatedTokenAddress(
    sourceOwner.publicKey,
    marketUSDC.quoteMintAddress,
  );
  await marketUSDC.settleFunds(
    connection,
    sourceOwner,
    openOrder,
    sourceTokenKey,
    sourceUSDCKey,
  );

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

  // Buy the to the corresponding tokens with the source USDC in correct ratios
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
  let poolAssetMarkets: Array<Market> = [];
  let poolTokenAmount = 0;
  for (let i = 0; i < poolAssets.length; i++) {
    let poolAssetSymbol =
      TOKEN_MINTS[
        TOKEN_MINTS.map(t => t.address.toString()).indexOf(
          poolAssets[i].mintAddress.toString(),
        )
      ].name;
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
    await assetMarket.placeOrder(connection, {
      owner: sourceOwner,
      payer: payer.publicKey,
      side: 'buy',
      price: assetMidPrice,
      size: assetAmountToBuy,
      orderType: 'ioc',
    });
  }

  // TODO potentially wait for match
  await sleep(30 * 1000);

  // Settle the USDC to Poolassets transfers
  for (let i = 0; i < poolAssets.length; i++) {
    let assetMarket = poolAssetMarkets[i];
    let openOrders = await assetMarket.findOpenOrdersAccountsForOwner(
      connection,
      sourceOwner.publicKey,
    );
    let openOrder =
      openOrders[
        openOrders
          .map(o => o.market.toString())
          .indexOf(assetMarket.address.toString())
      ];
    let sourceUSDCKey = await findAssociatedTokenAddress(
      sourceOwner.publicKey,
      marketUSDC.quoteMintAddress,
    );
    await assetMarket.settleFunds(
      connection,
      sourceOwner,
      openOrder,
      sourceUSDCKey,
      sourceAssetKeys[i],
    );
  }

  // If nonexistent, create the source owner associated address to receive the pooltokens
  let instructions: Array<TransactionInstruction> = [];
  let targetPoolTokenKey = await findAssociatedTokenAddress(
    sourceOwner.publicKey,
    poolMintKey,
  );
  let targetInfo = await connection.getAccountInfo(targetPoolTokenKey);
  if (Object.is(targetInfo, null)) {
    instructions.push(
      await createAssociatedTokenAccount(
        SystemProgram.programId,
        payer.publicKey,
        sourceOwner.publicKey,
        poolMintKey,
      ),
    );
  }

  // Do the effective deposit
  let depositTxInstruction = depositInstruction(
    TOKEN_PROGRAM_ID,
    bonfidaBotProgramId,
    poolMintKey,
    poolKey,
    poolAssetKeys,
    targetPoolTokenKey,
    sourceOwner.publicKey,
    sourceAssetKeys,
    [poolSeed],
    // @ts-ignore
    new Numberu64(poolTokenAmount),
  );
  instructions.push(depositTxInstruction);
  return await signAndSendTransactionInstructions(
    connection,
    [sourceOwner],
    payer,
    instructions,
  );
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

// TODO 2nd layer bindings: iterative deposit + settle all(find open orders by owner) + settle&redeem + cancelall + create_easy
// TODO adapt bindings to Elliott push in state
