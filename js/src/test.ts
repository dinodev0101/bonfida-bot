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
  } from './utils';
  import { OrderSide, OrderType, PoolAsset, PoolHeader, PoolStatus, SelfTradeBehavior, unpack_assets } from './state';
  import { assert } from 'console';
  import bs58 from 'bs58';
  import * as crypto from "crypto";
  import { Order } from '@project-serum/serum/lib/market';
  import { cancelOrder, createOrder, createPool, deposit, redeem, settleFunds } from './main';
import { getPoolsSeedsBySigProvider } from './secondary_bindings';


const test = async (): Promise<void> => {
  
    // Connection
    const ENDPOINTS = {
      mainnet: 'https://solana-api.projectserum.com',
      devnet: 'https://devnet.solana.com',
    };
    const connection = new Connection(ENDPOINTS.mainnet);
  
    const BONFIDABOT_PROGRAM_ID: PublicKey = new PublicKey(
      "GCv8mMWTwpYCNh6xbMPsx2Z7yKrjCC7LUz6nd3cMZokB", //'4n5939p99bGJRCVPtf2kffKftHRjw6xRXQPcozsVDC77', old GCv8mMWTwpYCNh6xbMPsx2Z7yKrjCC7LUz6nd3cMZokB new
    );
  
    const SERUM_PROGRAM_ID: PublicKey = new PublicKey(
      "EUqojwWA2rd19FZrzeBncJsm38Jm1hEhE3zsmX3bRc2o",
    );
    
    const FIDA_KEY: PublicKey = new PublicKey(
      "EchesyfXePKdLtoiZSL8pBe8Myagyy8ZRqsACNCFGnvp",
    );
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
    const sourceOwnerAccount = new Account([209,138,118,246,5,217,67,204,37,161,220,18,155,172,128,23,242,70,137,170,6,59,58,212,25,44,166,224,141,41,91,65,8,38,24,142,233,90,158,76,163,107,196,192,78,223,10,102,45,91,195,145,5,138,109,51,78,187,243,50,190,254,210,179]);
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

    // Create Pool
    let [poolInfo, createInstructions] = await createPool(
      connection,
      BONFIDABOT_PROGRAM_ID,
      SERUM_PROGRAM_ID,
      sourceOwnerAccount,
      sourceAssetKeys,
      signalProviderAccount.publicKey,
      [2000000, 1000000],
      10,
      // @ts-ignore
      new Numberu16(1),
      [marketInfo.address],
      payerAccount
    );
  
    // Deposit into Pool
    let depositTxInstructions = await deposit(
      connection,
      BONFIDABOT_PROGRAM_ID,
      sourceOwnerAccount.publicKey,
      sourceAssetKeys,
      // @ts-ignore
      new Numberu64(1000000),
      [poolInfo.seed],
      payerAccount
    );
  
    let [openOrderAccount, createPoolTxInstructions] = await createOrder(
      connection,
      BONFIDABOT_PROGRAM_ID,
      SERUM_PROGRAM_ID,
      poolInfo.seed,
      marketInfo.address,
      OrderSide.Ask,
      // @ts-ignore
      new Numberu64(1<<63),
      // @ts-ignore
      new Numberu16(1<<15),
      OrderType.Limit,
      // @ts-ignore
      new Numberu64(0),
      SelfTradeBehavior.DecrementTake,
      null, // Self referring
      payerAccount.publicKey
    );

    let firstInstructions = createInstructions
      .concat(depositTxInstructions)
      .concat(createPoolTxInstructions)
    await signAndSendTransactionInstructions(
      connection,
      [sourceOwnerAccount, signalProviderAccount],
      payerAccount,
      firstInstructions
    );
  

    let cancelOrderTxInstruction = await cancelOrder(
      connection,
      BONFIDABOT_PROGRAM_ID,
      SERUM_PROGRAM_ID,
      poolInfo.seed,
      marketInfo.address,
      openOrderAccount.publicKey
    );

    let sourcePoolTokenKey = await findAssociatedTokenAddress(
      poolInfo.address,
      poolInfo.mintKey
    );

    let settleFundsTxInstructions = await settleFunds(
        connection,
        BONFIDABOT_PROGRAM_ID,
        SERUM_PROGRAM_ID,
        poolInfo.seed,
        marketInfo.address,
        openOrderAccount.publicKey,
        null
    );
    
    let redeemTxInstruction = await redeem(
      connection,
      BONFIDABOT_PROGRAM_ID,
      sourceOwnerAccount.publicKey,
      sourcePoolTokenKey,
      sourceAssetKeys,
      [poolInfo.seed],
      // @ts-ignore
      new Numberu64(1000000)
    );

    let secondInstructions = cancelOrderTxInstruction
      .concat(settleFundsTxInstructions)
      .concat(redeemTxInstruction)
    await signAndSendTransactionInstructions(
      connection,
      [signalProviderAccount, sourceOwnerAccount],
      payerAccount,
      firstInstructions
    );
      
    let fetchedSeeds = await getPoolsSeedsBySigProvider(
      connection,
      BONFIDABOT_PROGRAM_ID,
      signalProviderAccount.publicKey
    );
    console.log(fetchedSeeds.map(seed => bs58.encode(seed)));
  
    // Add an instruction that will result in an error for testing
    /*
    Results in:
    Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL invoke [1]
      Program log: Transfer 2039280 lamports to the associated token account
      Program 11111111111111111111111111111111 invoke [2]
      Program 11111111111111111111111111111111 success
      Program log: Allocate space for the associated token account
      Program 11111111111111111111111111111111 invoke [2]
      Program 11111111111111111111111111111111 success
      Program log: Assign the associated token account to the SPL Token program
      Program 11111111111111111111111111111111 invoke [2]
      Program 11111111111111111111111111111111 success
      Program log: Initialize the associated token account
      Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]
      Program log: Instruction: InitializeAccount
      Program log: Error: Invalid Mint
      Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3469 of 169960 compute units
      Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA failed: custom program error: 0x2
      Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL consumed 33509 of 200000 compute units
      Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL BPF VM error: custom program error: 0x2
      Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL failed: custom program error: 0x2
    TODO remove
    */
    // let crashTxInstruction = await createAssociatedTokenAccount(
    //   SystemProgram.programId,
    //   payerAccount.publicKey,
    //   sourceOwnerAccount.publicKey,
    //   sourceOwnerAccount.publicKey
    // );
  
    // let instructions: TransactionInstruction[] = depositInstructions;
    // instructions = instructions.concat(createOrderTxInstructions);
    // instructions = instructions.concat(settleFundsTxInstructions);
    // instructions = instructions.concat(cancelOrderTxInstruction);
    // instructions = instructions.concat(redeemTxInstruction);
    // // instructions.push(crashTxInstruction);

  };
  
  test();
  