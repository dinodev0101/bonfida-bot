import {
    Account,
    PublicKey,
    SystemProgram,
    SYSVAR_INSTRUCTIONS_PUBKEY,
    SYSVAR_RENT_PUBKEY,
    TransactionInstruction,
    Connection,
    sendAndConfirmTransaction,
    SystemInstruction,
    CreateAccountParams,
  } from '@solana/web3.js';
import { TOKEN_PROGRAM_ID, Token, AccountLayout } from '@solana/spl-token';
import { EVENT_QUEUE_LAYOUT, Market, MARKETS, REQUEST_QUEUE_LAYOUT, OpenOrders } from '@project-serum/serum';
import {
  cancelOrderInstruction,
  createInstruction,
  createOrderInstruction,
  depositInstruction,
  initInstruction,
  Instruction,
  settleFundsInstruction,
} from './instructions';
import {
  signAndSendTransactionInstructions,
  findAssociatedTokenAddress,
  createAssociatedTokenAccount,
  getAccountFromSeed,
  Numberu64,
  Numberu16,
  getMarketData,
  Numberu128,
  sleep,
  getMidPrice,
} from './utils';
import { OrderSide, OrderType, PoolAsset, PoolHeader, PoolStatus, PoolStatusID, SelfTradeBehavior, unpack_assets } from './state';
import { assert } from 'console';
import bs58 from 'bs58';
import * as crypto from "crypto";
import { Order } from '@project-serum/serum/lib/market';
import { BONFIDABOT_PROGRAM_ID, cancelOrder, collectFees, createOrder, createPool, deposit, ENDPOINTS, redeem, SERUM_PROGRAM_ID, settleFunds } from './main';
import { fetchPoolBalances, fetchPoolInfo, getPoolsSeedsBySigProvider, singleTokenDeposit } from './secondary_bindings';
import { SOURCE_OWNER_ACCOUNT } from './secret';


const test = async (): Promise<void> => {
  
    const connection = new Connection(ENDPOINTS.mainnet);
  
    const FIDA_VAULT_KEY: PublicKey = new PublicKey(
      "Hoh5ocM73zN8RrjfgkY7SwdMnj3CXy3kDZpK4A5nLg3k",
    );
  
    const USDC_KEY: PublicKey = new PublicKey(
      "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    );
    const USDC_VAULT_KEY: PublicKey = new PublicKey(
      "4XzLuVzzSbYYq1ZJvoWaUWm5kAHZNEuaxqLKNPoYUHPi",
    );

    // Accounts to use for test
    const sourceOwnerAccount = SOURCE_OWNER_ACCOUNT;
    //Pubkey: YoxKe1BcnqEfCd5nTQR9VqNaYvYwLsZfFkiUZXHXpve (id_mainnet.json)
    const sourceAssetKeys = [
      new PublicKey("143edbvX6YWnz8epG2q5Meu9Bdu6J6czm6z6Aa6wonQ6"),
      new PublicKey("G9GeWZvm6LJN9yCqyUeyicScvkaJrKgkKGs5JZQXHDgy")
    ];
    const signalProviderAccount = sourceOwnerAccount;
    const payerAccount = sourceOwnerAccount;
  
    // Get FIDA to USDC market
    let marketInfo = MARKETS[MARKETS.map(m => {return m.name}).lastIndexOf("FIDA/USDC")];
    if (marketInfo.deprecated) {throw "Chosen Market is deprecated"};
    let marketData = await getMarketData(connection, marketInfo.address);

    // // Create Pool
    // let [poolSeed, createInstructions] = await createPool(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   SERUM_PROGRAM_ID,
    //   sourceOwnerAccount.publicKey,
    //   sourceAssetKeys,
    //   signalProviderAccount.publicKey,
    //   [500000, 1000000],
    //   10,
    //   // @ts-ignore
    //   new Numberu16(1),
    //   [marketInfo.address],
    //   payerAccount.publicKey,
    //   // @ts-ignore
    //   new Numberu64(604800),
    //   // @ts-ignore
    //   new Numberu16(1 << 13)
    // );

    // await signAndSendTransactionInstructions(
    //   connection,
    //   [sourceOwnerAccount],
    //   payerAccount,
    //   createInstructions
    // );
    // console.log("Created Pool")
    // await sleep(5 * 1000);
    // // Needs to sleep longer than this ?

    let poolSeed = bs58.decode("3vfRZF75MoYnhbne399ASdkG7JNXJQ5wZ3AYE2kDJwnn");

    let poolInfo = await fetchPoolInfo(connection, BONFIDABOT_PROGRAM_ID, poolSeed);

    // Deposit into Pool
    let depositTxInstructions = await deposit(
      connection,
      BONFIDABOT_PROGRAM_ID,
      sourceOwnerAccount.publicKey,
      sourceAssetKeys,
      // @ts-ignore
      new Numberu64(1000000),
      [poolInfo.seed],
      payerAccount.publicKey
    );

    await signAndSendTransactionInstructions(
      connection,
      [sourceOwnerAccount],
      payerAccount,
      depositTxInstructions
    );
    console.log("Deposited into Pool")
    // await sleep(5 * 1000);
  
    // let [openOrderAccount, createPoolTxInstructions] = await createOrder(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   SERUM_PROGRAM_ID,
    //   poolInfo.seed,
    //   marketInfo.address,
    //   OrderSide.Ask,
    //   // @ts-ignore
    //   new Numberu64(1<<63),
    //   // @ts-ignore
    //   new Numberu16(1<<15),
    //   OrderType.Limit,
    //   // @ts-ignore
    //   new Numberu64(0),
    //   SelfTradeBehavior.DecrementTake,
    //   null, // Self referring
    //   payerAccount.publicKey
    // );

    // await signAndSendTransactionInstructions(
    //   connection,
    //   [openOrderAccount, signalProviderAccount],
    //   payerAccount,
    //   createPoolTxInstructions
    // );
    // console.log("Created Order for Pool")
    // await sleep(5 * 1000);
  
    // let cancelOrderTxInstruction = await cancelOrder(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   SERUM_PROGRAM_ID,
    //   poolInfo.seed,
    //   marketInfo.address,
    //   openOrderAccount.publicKey
    // );

    // await signAndSendTransactionInstructions(
    //   connection,
    //   [signalProviderAccount],
    //   payerAccount,
    //   cancelOrderTxInstruction
    // );
    // console.log("Cancelled Order")
    // await sleep(5 * 1000);

    // let sourcePoolTokenKey = await findAssociatedTokenAddress(
    //   sourceOwnerAccount.publicKey,
    //   poolInfo.mintKey
    // );

    // let settleFundsTxInstructions = await settleFunds(
    //     connection,
    //     BONFIDABOT_PROGRAM_ID,
    //     SERUM_PROGRAM_ID,
    //     poolInfo.seed,
    //     marketInfo.address,
    //     openOrderAccount.publicKey,
    //     null
    // );

    // await signAndSendTransactionInstructions(
    //   connection,
    //   [],
    //   payerAccount,
    //   settleFundsTxInstructions
    // );
    // console.log("Settled Funds")
    // await sleep(5 * 1000);
    

    // let redeemTxInstruction = await redeem(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   sourceOwnerAccount.publicKey,
    //   sourcePoolTokenKey,
    //   sourceAssetKeys,
    //   [poolInfo.seed],
    //   // @ts-ignore
    //   new Numberu64(400000)
    // );
    
    // await signAndSendTransactionInstructions(
    //   connection,
    //   [sourceOwnerAccount],
    //   payerAccount,
    //   redeemTxInstruction
    // );
    // console.log("Redeemed out of Pool")
     
    
    // let collectFeesTxInstruction = await collectFees(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   [poolInfo.seed]
    // );
    
    // await signAndSendTransactionInstructions(
    //   connection,
    //   [sourceOwnerAccount],
    //   payerAccount,
    //   collectFeesTxInstruction
    // );
    // console.log("Redeemed out of Pool")
     

    //////////////////////////////////////////////

    // let poolSeed = bs58.decode("3vfRZF75MoYnhbne399ASdkG7JNXJQ5wZ3AYE2kDJwnn");

    // singleTokenDeposit(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   sourceOwnerAccount,
    //   sourceAssetKeys[0],
    //   1,
    //   poolSeed,
    //   payerAccount
    // )

    //////////////////////////////////////////////

   
    // let fetchedSeeds = await getPoolsSeedsBySigProvider(
    //   connection,
    //   BONFIDABOT_PROGRAM_ID,
    //   undefined
    // );
    // console.log();
    // console.log("Seeds of existing pools:")
    // console.log(fetchedSeeds.map(seed => bs58.encode(seed)));
    // console.log();
    
    // let poolSeed = bs58.decode("GPCLUeYJHMK3qA4oQZ2WqCRWYFt7987Yg2dtaQSFd8ow");
    
    // let poolInfo = await fetchPoolInfo(connection, BONFIDABOT_PROGRAM_ID, poolSeed);
    // console.log("Pool Info:")
    // console.log({
    //     address: poolInfo.address.toString(),
    //     serumProgramId: poolInfo.serumProgramId.toString(),
    //     seed: bs58.encode(poolInfo.seed),
    //     signalProvider: poolInfo.signalProvider.toString(),
    //     status: [PoolStatusID[poolInfo.status[0]], poolInfo.status[1]],
    //     feeRatio: Number(poolInfo.feeRatio),
    //     feePeriod: Number(poolInfo.feePeriod),
    //     mintKey: poolInfo.mintKey.toString(),
    //     assetMintkeys: poolInfo.assetMintkeys.map(asset => asset.toString()),
    //     authorizedMarkets: poolInfo.authorizedMarkets.map(market => market.toString())
    // });
    // console.log();

    // let poolBalances = await fetchPoolBalances(connection, BONFIDABOT_PROGRAM_ID, poolSeed);
    // console.log("Total Pooltokens", poolBalances[0]);
    // console.log("Pool Balances:")
    // console.log(poolBalances[1].map(b => { return {
    //   mint: b.mint.toString(),
    //   amount: b.tokenAmount.amount
    // }}));

  };
  
  test();
  