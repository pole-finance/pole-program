// Boilerplate utils to bootstrap an orderbook for testing on a localnet.
// not super relevant to the point of the example, though may be useful to
// include into your own workspace for testing.
//
// TODO: Modernize all these apis. This is all quite clunky.


import { Token, TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { TokenInstructions, Market, DexInstructions } from "@project-serum/serum";
import { Connection, PublicKey, Transaction, SystemProgram, Keypair, Account, Commitment} from "@solana/web3.js";
import { createMintAndVault } from "@project-serum/common";
import {Provider, BN } from "@project-serum/anchor";

const DEX_PID = new PublicKey("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin");

export async function setupAMarket({ provider } : {provider: Provider;}): Promise<
{
  marketPortUSDC: Market;
  marketMaker: {tokens: Map<string, PublicKey>; account: Keypair;}
  portMint: PublicKey;
  usdcMint: PublicKey;
  portTokenAccount: PublicKey;
  usdcTokenAccount: PublicKey;
}> {
  // Setup mints with initial tokens owned by the provider.
  const decimals = 6;
  const [portMint, portTokenAccount] = await createMintAndVault(
    provider,
    new BN(1000000000000000),
    undefined,
    decimals
  );
  const [usdcMint, usdcTokenAccount] = await createMintAndVault(
    provider,
    new BN(1000000000000000),
    undefined,
    decimals
  );

  // Create a funded account to act as market maker.
  const amount = 100000 * 10 ** decimals;
  const marketMaker = await fundAccount({
    provider,
    mints: [
      { tokenAccount: portTokenAccount, mint: portMint, amount, decimals },
      { tokenAccount: usdcTokenAccount, mint: usdcMint, amount, decimals },
    ],
  });

  // Setup PORT/USDC market with resting orders.
  const asks = [
    [6.041, 755.8],
    [6.051, 7772.3],
    [6.055, 775.4],
    [6.067, 615.7],
    [6.077, 390.0],
    [6.09, 524.0],
    [6.11, 436.3],
    [6.133, 300.0],
    [6.167, 987.8],
  ];
  const bids = [
    [6.004, 3865.5],
    [5.995, 3125.9],
    [5.987, 36.2],
    [5.978, 315.3],
    [5.965, 382.8],
    [5.961, 995.4],
  ];

  const baseTokenAccount = marketMaker.tokens.get(portMint.toString());
  const quoteTokenAccount =  marketMaker.tokens.get(usdcMint.toString());

  if (!baseTokenAccount || !quoteTokenAccount) {
    throw new Error("base or quote token not found");
  }

  const marketPortUSDC = await setupMarket({
    baseMint: portMint,
    quoteMint: usdcMint,
    marketMaker: {
      account: marketMaker.account,
      baseTokenAccount,
      quoteTokenAccount,
    },
    bids,
    asks,
    provider,
  });

  return {
    marketPortUSDC,
    marketMaker,
    portMint,
    usdcMint,
    portTokenAccount,
    usdcTokenAccount,
  };
}

export async function fundAccount({ provider, mints }:
  {
    provider: Provider; 
    mints: {
      tokenAccount: PublicKey;
      mint: PublicKey;
      amount: number;
      decimals: number;
    }[]
}): Promise<{tokens: Map<string, PublicKey>; account: Keypair;}> {
  const marketMakerAccount = new Keypair();

  const marketMaker = {
    tokens: new Map<string, PublicKey>(),
    account: marketMakerAccount,
  };

  // Transfer lamports to market maker.
  await provider.send(
    (() => {
      const tx = new Transaction();
      tx.add(
        SystemProgram.transfer({
          fromPubkey: provider.wallet.publicKey,
          toPubkey: marketMakerAccount.publicKey,
          lamports: 100000000000,
        })
      );
      return tx;
    })()
  );

  // Transfer SPL tokens to the market maker.
  for (let k = 0; k < mints.length; k += 1) {
    // eslint-disable-next-line
    const { mint, tokenAccount, amount, decimals } = mints[k]!;

    const associateTokenAccount = await Token.getAssociatedTokenAddress(
      ASSOCIATED_TOKEN_PROGRAM_ID,
      TOKEN_PROGRAM_ID,
      mint,
      marketMakerAccount.publicKey,
    );

    await provider.send(
      (() => {
        const tx = new Transaction();
        tx.add(
          Token.createAssociatedTokenAccountInstruction(
            ASSOCIATED_TOKEN_PROGRAM_ID,
            TOKEN_PROGRAM_ID,
            mint,
            associateTokenAccount,
            marketMakerAccount.publicKey,
            provider.wallet.publicKey
          ),
          Token.createTransferCheckedInstruction(
            TOKEN_PROGRAM_ID,
            tokenAccount,
            mint,
            associateTokenAccount,
            provider.wallet.publicKey,
            [],
            amount,
            decimals
          ),
        );
        return tx;
      })()
    );

    marketMaker.tokens.set(mint.toString(), associateTokenAccount);
  }

  return marketMaker;
}

export async function setupMarket({
  provider,
  marketMaker,
  baseMint,
  quoteMint,
  bids,
  asks,
}: {
  provider: Provider;
  marketMaker: {
    account: Keypair;
    baseTokenAccount: PublicKey;
    quoteTokenAccount: PublicKey;
  };
  baseMint: PublicKey;
  quoteMint: PublicKey;
  bids: number[][];
  asks: number[][];
}) {
  const marketAPublicKey = await listMarket({
    connection: provider.connection,
    wallet: provider.wallet,
    baseMint: baseMint,
    quoteMint: quoteMint,
    baseLotSize: 100000,
    quoteLotSize: 100,
    dexProgramId: DEX_PID,
    feeRateBps: 0,
  });
  const MARKET_A_USDC = await Market.load(
    provider.connection,
    marketAPublicKey,
    { commitment: "recent" },
    DEX_PID
  );
  const ownerAccount = new Account(marketMaker.account.secretKey);
  for (let k = 0; k < asks.length; k += 1) {
    const ask = asks[k];
    if (!ask || ask.length < 2) {
      throw new Error("ask format not correct");
    }
    const [askPrice, askSize] = ask;
    if (!askPrice || !askSize) {
      throw new Error("ask price or size not correct");
    }

    const {
      transaction,
      signers,
    } = await MARKET_A_USDC.makePlaceOrderTransaction(provider.connection, {
      owner: ownerAccount,
      payer: marketMaker.baseTokenAccount,
      side: "sell",
      price: askPrice,
      size: askSize,
      orderType: "postOnly",
      clientId: undefined,
      openOrdersAddressKey: undefined,
      openOrdersAccount: undefined,
      feeDiscountPubkey: null,
      selfTradeBehavior: "abortTransaction",
    });
    await provider.send(transaction, signers.concat(ownerAccount));
  }

  for (let k = 0; k < bids.length; k += 1) {
    const bid = bids[k];
    if (!bid || bid.length < 2) {
      throw new Error("ask format not correct");
    }

    const [bidPrice, bidSize] = bid;
    if (!bidPrice || !bidSize) {
      throw new Error("bid price or size not correct");
    }

    const {
      transaction,
      signers,
    } = await MARKET_A_USDC.makePlaceOrderTransaction(provider.connection, {
      owner: ownerAccount,
      payer: marketMaker.quoteTokenAccount,
      side: "buy",
      price: bidPrice,
      size: bidSize,
      orderType: "postOnly",
      clientId: undefined,
      openOrdersAddressKey: undefined,
      openOrdersAccount: undefined,
      feeDiscountPubkey: null,
      selfTradeBehavior: "abortTransaction",
    });
    await provider.send(transaction, signers.concat(ownerAccount));
  }

  return MARKET_A_USDC;
}

export async function listMarket({
  connection,
  wallet,
  baseMint,
  quoteMint,
  baseLotSize,
  quoteLotSize,
  dexProgramId,
  feeRateBps,
}: {
  connection: Connection;
  wallet: AnchorWallet;
  baseMint: PublicKey;
  quoteMint: PublicKey;
  baseLotSize: number;
  quoteLotSize: number;
  dexProgramId: PublicKey;
  feeRateBps: number;
}) {
  const market = new Keypair();
  const requestQueue = new Keypair();
  const eventQueue = new Keypair();
  const bids = new Keypair();
  const asks = new Keypair();
  const baseVault = new Keypair();
  const quoteVault = new Keypair();
  const quoteDustThreshold = new BN(100);

  const [vaultOwner, vaultSignerNonce] = await getVaultOwnerAndNonce(
    market.publicKey,
    dexProgramId
  );

  const tx1 = new Transaction();
  tx1.add(
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: baseVault.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(165),
      space: 165,
      programId: TOKEN_PROGRAM_ID,
    }),
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: quoteVault.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(165),
      space: 165,
      programId: TOKEN_PROGRAM_ID,
    }),
    TokenInstructions.initializeAccount({
      account: baseVault.publicKey,
      mint: baseMint,
      owner: vaultOwner,
    }),
    TokenInstructions.initializeAccount({
      account: quoteVault.publicKey,
      mint: quoteMint,
      owner: vaultOwner,
    })
  );

  const tx2 = new Transaction();
  tx2.add(
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: market.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(
        Market.getLayout(dexProgramId).span
      ),
      space: Market.getLayout(dexProgramId).span,
      programId: dexProgramId,
    }),
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: requestQueue.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(5120 + 12),
      space: 5120 + 12,
      programId: dexProgramId,
    }),
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: eventQueue.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(262144 + 12),
      space: 262144 + 12,
      programId: dexProgramId,
    }),
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: bids.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(65536 + 12),
      space: 65536 + 12,
      programId: dexProgramId,
    }),
    SystemProgram.createAccount({
      fromPubkey: wallet.publicKey,
      newAccountPubkey: asks.publicKey,
      lamports: await connection.getMinimumBalanceForRentExemption(65536 + 12),
      space: 65536 + 12,
      programId: dexProgramId,
    }),
    DexInstructions.initializeMarket({
      market: market.publicKey,
      requestQueue: requestQueue.publicKey,
      eventQueue: eventQueue.publicKey,
      bids: bids.publicKey,
      asks: asks.publicKey,
      baseVault: baseVault.publicKey,
      quoteVault: quoteVault.publicKey,
      baseMint,
      quoteMint,
      baseLotSize: new BN(baseLotSize),
      quoteLotSize: new BN(quoteLotSize),
      feeRateBps,
      vaultSignerNonce,
      quoteDustThreshold,
      programId: dexProgramId,
    })
  );

  const signedTransactions = await signTransactions({
    transactionsAndSigners: [
      { transaction: tx1, signers: [baseVault, quoteVault] },
      {
        transaction: tx2,
        signers: [market, requestQueue, eventQueue, bids, asks],
      },
    ],
    wallet,
    connection,
  });
  for (const signedTransaction of signedTransactions) {
    await sendAndConfirmRawTransaction(
      connection,
      signedTransaction.serialize()
    )
  }
  const acc = await connection.getAccountInfo(market.publicKey);

  if (acc === null) {
    throw new Error("market not created");
  }

  return market.publicKey;
}

async function signTransactions({
  transactionsAndSigners,
  wallet,
  connection,
} : {
  transactionsAndSigners: {transaction: Transaction; signers: Keypair[]}[];
  wallet: AnchorWallet;
  connection: Connection;
}) {
  const blockhash = (await connection.getRecentBlockhash("max")).blockhash;
  transactionsAndSigners.forEach(({ transaction, signers = [] }) => {
    transaction.recentBlockhash = blockhash;
    transaction.setSigners(
      wallet.publicKey,
      ...signers.map((s) => s.publicKey)
    );
    if (signers.length > 0) {
      transaction.partialSign(...signers);
    }
  });
  return await wallet.signAllTransactions(
    transactionsAndSigners.map(({ transaction }) => transaction)
  );
}

async function sendAndConfirmRawTransaction(
  connection: Connection,
  raw : Buffer | Uint8Array | Array<number>,
  commitment?: Commitment
) {
  const tx = await connection.sendRawTransaction(raw, {
    skipPreflight: true,
  });
  return await connection.confirmTransaction(tx, commitment);
}

async function getVaultOwnerAndNonce(marketPublicKey: PublicKey, dexProgramId: PublicKey = DEX_PID) {
  const nonce = new BN(0);
  while (nonce.toNumber() < 255) {
    try {
      const vaultOwner = await PublicKey.createProgramAddress(
        [marketPublicKey.toBuffer(), nonce.toArrayLike(Buffer, "le", 8)],
        dexProgramId
      );
      return [vaultOwner, nonce];
    } catch (e) {
      nonce.iaddn(1);
    }
  }
  throw new Error("Unable to find nonce");
}

export interface AnchorWallet {
  signTransaction(tx: Transaction): Promise<Transaction>;
  signAllTransactions(txs: Transaction[]): Promise<Transaction[]>;
  publicKey: PublicKey;
}
