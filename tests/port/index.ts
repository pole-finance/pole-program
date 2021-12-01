import { Provider } from "@project-serum/anchor";
import { getTokenAccount } from "@project-serum/common";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { Keypair, PublicKey, Transaction } from "@solana/web3.js";
import { LENDING_MARKET_LEN, PORT_LENDING, PORT_STAKING, RESERVE_LEN, SERUM_DEX_PROGRAM_ID, STAKING_POOL_LEN, TOKEN_ACCOUNT_LEN, TOKEN_MINT_LEN } from "../constants";
import { Market } from '@project-serum/serum';
import { Reserve } from "../state/reserve";
import { createAccount } from "../utils";
import BN from "bn.js";
import { initLendingMarketInstruction, initReserveInstruction, initStakingPool } from "@port.finance/port-sdk";

export interface ReserveState {
  address: PublicKey;
  liquiditySupplyPubkey: PublicKey;
  collateralMintAccount: PublicKey;
  collateralSupplyTokenAccount: PublicKey;
  liquidityFeeReceiver: PublicKey;
  useCollateralAccount: PublicKey;
}

export interface PortAccounts {
  userLpWallet: PublicKey;
  lpMint: PublicKey;
  reserveLpWallet: PublicKey;
  liquidityMint: PublicKey;
  liquidityReserve: PublicKey;
  obligation: PublicKey;
  stakeAccount: PublicKey;
  stakingPool: PublicKey;
  lendingMarket: PublicKey;
  portLendingProgram: PublicKey;
  portStakingProgram: PublicKey;
  reserve: PublicKey;
  reserveFee: PublicKey;
  lendingMarketAuthority: PublicKey;
  reserveLiquidityWallet: PublicKey;
}

export async function generatePortAccounts(
    // eslint-disable-next-line
  liquidityMint: PublicKey,reserveAddr: PublicKey, reserve: Reserve, polePool: any): Promise<PortAccounts> {
  const [lendingMarketAuthority] = await PublicKey.findProgramAddress(
    [reserve.lendingMarket.toBuffer()],
    PORT_LENDING
  );

  return {
    userLpWallet: polePool.portConfig.portLpSupply,
    lpMint: reserve.collateral.mintPubkey,
    reserveLpWallet: reserve.collateral.supplyPubkey,
    liquidityMint: liquidityMint,
    reserveLiquidityWallet: reserve.liquidity.supplyPubkey,
    obligation: polePool.portConfig.obligation,
    stakeAccount: polePool.portConfig.stakeAccount,
    stakingPool: reserve.config.stakingPool,
    portLendingProgram: polePool.portConfig.portLendingProgram,
    portStakingProgram: polePool.portConfig.portStakingProgram,
    reserve: reserveAddr,
    reserveFee: reserve.liquidity.feeReceiver,
    lendingMarket: reserve.lendingMarket,
    lendingMarketAuthority: lendingMarketAuthority,
    liquidityReserve: reserve.liquidity.supplyPubkey,
  }
}

export interface SerumAccounts {
  market: PublicKey,
  openOrders: PublicKey,
  requestQueue: PublicKey,
  eventQueue: PublicKey,
  bids: PublicKey,
  asks: PublicKey,
  coinVault: PublicKey,
  pcVault: PublicKey,
  vaultSigner: PublicKey
}

// eslint-disable-next-line
export async function generateSerumAccounts(provider: Provider, marketAddress: PublicKey, polePool: any): Promise<SerumAccounts> {
  const market = await Market.load(provider.connection, marketAddress, {}, SERUM_DEX_PROGRAM_ID);
  const vaultSigner = await PublicKey.createProgramAddress(
    // eslint-disable-next-line
    // @ts-ignore
    [marketAddress.toBuffer(), (new BN(market._decoded.vaultSignerNonce)).toArrayLike(Buffer, "le", 8)], SERUM_DEX_PROGRAM_ID
  );

  return {
    market: marketAddress,
    openOrders: polePool.serumConfig.portOpenOrders,
    asks: market.asksAddress,
    bids: market.bidsAddress,
    // eslint-disable-next-line
    // @ts-ignore
    coinVault: market._decoded.baseVault,
    // eslint-disable-next-line
    // @ts-ignore
    pcVault: market._decoded.quoteVault,
    vaultSigner: vaultSigner,
    // eslint-disable-next-line
    // @ts-ignore
    requestQueue: market._decoded.requestQueue,
    // eslint-disable-next-line
    // @ts-ignore
    eventQueue: market._decoded.eventQueue,
  }
}

export async function createLendingMarket(provider: Provider): Promise<Keypair> {
  const lendingMarket = await createAccount(
    provider,
    LENDING_MARKET_LEN,
    PORT_LENDING
  );
  await provider.send(
    (() => {
      const tx = new Transaction();
      tx.add(
        initLendingMarketInstruction(
          provider.wallet.publicKey,
          Buffer.from("USD\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0", 'ascii'),
          lendingMarket.publicKey,
        )
      );
      return tx;
    })(),
    []
  );
  return lendingMarket;
}

export async function createStakingPool(provider: Provider, sourceTokenAccount: PublicKey, sourceTokenMint: PublicKey, owner: PublicKey, portAmount: number) {
  const stakingPool = await createAccount(
    provider,
    STAKING_POOL_LEN,
    PORT_STAKING
  );

  const rewardTokenAccount = await createAccount(
    provider,
    TOKEN_ACCOUNT_LEN,
    TOKEN_PROGRAM_ID
  );

  const [stakingProgramAuthority, nounce] = await PublicKey.findProgramAddress(
    [stakingPool.publicKey.toBuffer()], PORT_STAKING
  );

  await provider.send(
    (() => {
      const tx = new Transaction();
      tx.add(
        initStakingPool(
          portAmount,
          51840,
          0,
          nounce,
          provider.wallet.publicKey,
          sourceTokenAccount,
          rewardTokenAccount.publicKey,
          stakingPool.publicKey,
          sourceTokenMint,
          stakingProgramAuthority,
          owner,
          provider.wallet.publicKey,
        )
      );
      return tx;
    })(),
    []
  );

  return {
    stakingPool,
    rewardTokenAccount
  }
}

export async function createDefaultReserve(
  provider: Provider, initialLiquidity: number | BN,
  sourceTokenWallet: PublicKey, lendingMarket: PublicKey,
  stakingPool: PublicKey): Promise<ReserveState> {
    const reserve = await createAccount(
      provider,
      RESERVE_LEN,
      PORT_LENDING
    );

    const collateralMintAccount = await createAccount(
      provider,
      TOKEN_MINT_LEN,
      TOKEN_PROGRAM_ID
    );

    const liquiditySupplyTokenAccount = await createAccount(
      provider,
      TOKEN_ACCOUNT_LEN,
      TOKEN_PROGRAM_ID
    );

    const collateralSupplyTokenAccount = await createAccount(
      provider,
      TOKEN_ACCOUNT_LEN,
      TOKEN_PROGRAM_ID
    );

    const userCollateralTokenAccount = await createAccount(
      provider,
      TOKEN_ACCOUNT_LEN,
      TOKEN_PROGRAM_ID
    );

    const liquidityFeeReceiver = await createAccount(
      provider,
      TOKEN_ACCOUNT_LEN,
      TOKEN_PROGRAM_ID
    );
    
    const [lendingMarketAuthority] = await PublicKey.findProgramAddress(
      [lendingMarket.toBuffer()],
      PORT_LENDING
    );

    const tokenAccount = await getTokenAccount(provider, sourceTokenWallet);

    const tx = new Transaction();

    tx.add(
      initReserveInstruction(
        initialLiquidity,
        1,
        new BN("1000000"),
        {
          optimalUtilizationRate: 80,
          loanToValueRatio: 80,
          liquidationBonus: 5,
          liquidationThreshold: 85,
          minBorrowRate: 0,
          optimalBorrowRate: 40,
          maxBorrowRate: 90,
          fees: {
            borrowFeeWad: new BN(10000000000000),
            flashLoanFeeWad: new BN(30000000000000),
            hostFeePercentage: 0
          },
          stakingPoolOption: 1,
          stakingPool: stakingPool,
        },
        sourceTokenWallet,
        userCollateralTokenAccount.publicKey,
        reserve.publicKey,
        tokenAccount.mint,
        liquiditySupplyTokenAccount.publicKey,
        liquidityFeeReceiver.publicKey,
        (Keypair.generate()).publicKey,
        collateralMintAccount.publicKey,
        collateralSupplyTokenAccount.publicKey,
        lendingMarket,
        lendingMarketAuthority,
        provider.wallet.publicKey,
        provider.wallet.publicKey,
      )
    );

    await provider.send(tx);

    return {
      address: reserve.publicKey,
      liquiditySupplyPubkey: liquiditySupplyTokenAccount.publicKey,
      collateralMintAccount: collateralMintAccount.publicKey,
      collateralSupplyTokenAccount: collateralSupplyTokenAccount.publicKey,
      liquidityFeeReceiver: liquidityFeeReceiver.publicKey,
      useCollateralAccount: userCollateralTokenAccount.publicKey,
    }

}