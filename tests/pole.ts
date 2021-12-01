import * as anchor from '@project-serum/anchor'
import {ASSOCIATED_TOKEN_PROGRAM_ID, Token, TOKEN_PROGRAM_ID} from '@solana/spl-token';
import {
  Keypair,
  PublicKey,
  SYSVAR_CLOCK_PUBKEY,
  SYSVAR_INSTRUCTIONS_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  Transaction
} from '@solana/web3.js';
import {
  PORT_LENDING,
  PORT_STAKING,
} from './constants';
import {setupAMarket} from './serum'
import {Program} from '@project-serum/anchor';
import {Pole, IDL} from '../target/types/pole';
import {createPolePool, fetchPolePool, fetchReserve, PoleState, SerumState, StakingPoolState} from './utils';
import {refreshReserveInstruction, refreshObligationInstruction} from '@port.finance/port-sdk';
import {getTokenAccount, sleep} from '@project-serum/common';
import {assert} from 'chai';
import {createDefaultReserve, createLendingMarket, createStakingPool, generatePortAccounts, generateSerumAccounts, ReserveState} from './port';
import Big from "big.js";


describe('pole', () => {

  const provider = anchor.Provider.local();

  // Accounts used to setup the orderbook.
  let serumOrderBook: SerumState;

  it('Set up PORT USDC Serum Market', async () => {
    serumOrderBook = await setupAMarket({provider});
  });

  let lendingMarket: Keypair;
  it('Set up PORT Lending Market', async () => {
    lendingMarket = await createLendingMarket(provider);
  })

  let stakingPoolState: StakingPoolState;
  it('Set up PORT Staking Pool', async () => {

    const [lendingMarketAuthority] = await PublicKey.findProgramAddress(
      [lendingMarket.publicKey.toBuffer()],
      PORT_LENDING
    );

    stakingPoolState = await createStakingPool(
      provider,
      serumOrderBook.portTokenAccount,
      serumOrderBook.portMint,
      lendingMarketAuthority,
      100000000000
    );

  });

  const initialLiquidity = 1;
  let reserveState: ReserveState;
  it('Set up PORT Reserve', async () => {
    reserveState = await createDefaultReserve(
      provider, initialLiquidity, serumOrderBook.usdcTokenAccount, lendingMarket.publicKey, stakingPoolState.stakingPool.publicKey);
  });


  const pole = new Program<Pole>(IDL, anchor.workspace.Pole.programId, provider);
  const poolName = "USDC";
  let poleState: PoleState;

  const deposit = (amount: number|string, isInit: boolean) =>
    async () => {
      const [polePool] = await PublicKey.findProgramAddress(
        [Uint8Array.from(poolName.split("").map(c => c.charCodeAt(0)))],
        pole.programId
      );
      let freshPolePool = await fetchPolePool(pole, polePool);
      const [poleAuthority] = await PublicKey.findProgramAddress(
        [],
        pole.programId
      );
      const poleLpTokenAccount = await Token.getAssociatedTokenAddress(
        ASSOCIATED_TOKEN_PROGRAM_ID,
        TOKEN_PROGRAM_ID,
        freshPolePool.genericConfig.lpMint,
        provider.wallet.publicKey,
      );
      let beforeUserLpAmount;
      if(!isInit) {
        await pole.provider.send(
          (() => {
            const initAssocTokenTx = new Transaction();
            initAssocTokenTx.add(
              Token.createAssociatedTokenAccountInstruction(
                ASSOCIATED_TOKEN_PROGRAM_ID,
                TOKEN_PROGRAM_ID,
                freshPolePool.genericConfig.lpMint,
                poleLpTokenAccount,
                provider.wallet.publicKey,
                provider.wallet.publicKey
              )
            )
            return initAssocTokenTx;
          })(),
          []
        );
        beforeUserLpAmount = new anchor.BN(0);
      } else {
        beforeUserLpAmount = (await getTokenAccount(provider, poleLpTokenAccount)).amount;
      }
      const reserve = await fetchReserve(provider, reserveState.address);
      const beforeReserveLiquidity = reserve.liquidity.availableAmount;

      const depositAccounts = {
        accounts: {
          polePool: polePool,
          poleAuthority: poleAuthority,
          poleLiquidityAccounts: {
            userLiquidityWallet: serumOrderBook.usdcTokenAccount,
            poleLiquidityWallet: freshPolePool.genericConfig.liquiditySupply
          },
          poleLpAccounts: {
            lpMint: freshPolePool.genericConfig.lpMint,
            userLpWallet: poleLpTokenAccount
          },
          portAccounts: await generatePortAccounts(
            serumOrderBook.usdcMint, reserveState.address, reserve, freshPolePool),
          userTransferAuthority: provider.wallet.publicKey,
          tokenProgram: freshPolePool.genericConfig.tokenProgram,
          clock: SYSVAR_CLOCK_PUBKEY
        },
        signers: []
      };
      const depositIxAmount = pole.instruction.depositLiquidity(
        new anchor.BN(amount),
        depositAccounts
      );
      const depositIxEmpty = pole.instruction.depositLiquidity(
        new anchor.BN(0),
        depositAccounts
      );
      const verifyIx = pole.instruction.verifyDeposit(
        {
          accounts: {
            polePool: polePool,
            transactionInfo: SYSVAR_INSTRUCTIONS_PUBKEY
          }
        }
      );
      const refreshReserveIx = refreshReserveInstruction(
        reserveState.address,
        null
      );
      const reserveToRefreshObligation = isInit? [reserveState.address] : [];
      const refreshObligationIx = refreshObligationInstruction(
        poleState.obligation,
        reserveToRefreshObligation,
        reserveToRefreshObligation
      );

      const tx = new Transaction();
      tx.add(
        verifyIx,
        refreshReserveIx,
        refreshObligationIx,
        depositIxAmount
      );


      for (let i = 1; i < freshPolePool.portConfig.portIterate; i += 1) {
        tx.add(
          refreshReserveIx,
          depositIxEmpty
        )
      }

      await pole.provider.send(tx);

      freshPolePool = await fetchPolePool(pole, polePool);
      const userLpWallet = await getTokenAccount(provider, poleLpTokenAccount);
      const parsedReserve = await fetchReserve(provider, reserveState.address);
      const reserveLiquidityGained = parsedReserve.liquidity.availableAmount - beforeReserveLiquidity;
      const userLpGained =  userLpWallet.amount.sub(beforeUserLpAmount);
      const lpAmount = freshPolePool.basicState.lpAmount;
      const liquidityShouldHave = new Big(parsedReserve.liquidity.availableAmount.toString()).div(new Big(lpAmount.toString())).mul(new Big(userLpGained.toString()));

      assert(liquidityShouldHave.lte(new Big( amount.toString())))
      assert(liquidityShouldHave.gte(new Big( amount.toString()).mul(0.95)), liquidityShouldHave.toString() + ", "  + amount.toString())
      assert(liquidityShouldHave.gte(new Big( amount.toString()).mul(0.9999)) || new Big(amount).lt(new Big("1000000")), liquidityShouldHave.toString() + ", "  + amount.toString())
      assert(
        // Initially reserve have 1 lamport liquidity
         beforeReserveLiquidity !== BigInt(0) || reserveLiquidityGained.toString() === userLpGained.toString(),
        'Available liquidity increase in PORT should match LP token increase ' + reserveLiquidityGained.toString() + " " + userLpGained.toString()
      );

    };

  const change_liquidity_cap =  (cap: number) => async () => {
    const [polePoolAddr] = await PublicKey.findProgramAddress(
      [Uint8Array.from(poolName.split("").map(c => c.charCodeAt(0)))],
      pole.programId
    );
    const changeCapAccs = {
      accounts : {
        polePool: polePoolAddr,
        owner: pole.provider.wallet.publicKey
      }
    };
    const changeCapIx = pole.instruction.changeLiquidityCap(
      new anchor.BN(cap),
      changeCapAccs,
    );
    const tx = new Transaction();
    tx.add(
      changeCapIx
    );
    await pole.provider.send(tx);
    const freshPolePool = await fetchPolePool(pole, polePoolAddr);
    assert(freshPolePool.genericConfig.liquidityCap.eq(new anchor.BN(cap)))
  };
  const claim_and_sell =
    async () => {
      const [polePoolAddr] = await PublicKey.findProgramAddress(
        [Uint8Array.from(poolName.split("").map(c => c.charCodeAt(0)))],
        pole.programId
      );
      const freshPolePool = await fetchPolePool(pole, polePoolAddr);
      const [poleAuthority] = await PublicKey.findProgramAddress(
        [],
        pole.programId
      );
      const [stakingProgramAuthority] = await anchor.web3.PublicKey.findProgramAddress(
        [stakingPoolState.stakingPool.publicKey.toBuffer()], PORT_STAKING
      );
      const beforeLiquidityAmount = new anchor.BN((await provider.connection.getTokenAccountBalance(freshPolePool.genericConfig.liquiditySupply)).value.amount);
      const claimAndSellAccs = {
        accounts: {
          polePool: polePoolAddr,
          poleAuthority: poleAuthority,
          liquiditySupply: freshPolePool.genericConfig.liquiditySupply,
          portSupply: freshPolePool.portConfig.portSupply,
          dexProgram: freshPolePool.serumConfig.dexProgram,
          swapProgram: freshPolePool.serumConfig.swapProgram,
          tokenProgram: TOKEN_PROGRAM_ID,
          portMint: serumOrderBook.portMint,
          marketAccounts: await generateSerumAccounts(
            provider, serumOrderBook.marketPortUSDC.address, freshPolePool
          ),
          portAccounts: {
            portStakingProgram: freshPolePool.portConfig.portStakingProgram,
            stakeAccount: poleState.stakeAccount,
            stakingPool: stakingPoolState.stakingPool.publicKey,
            rewardSupply: stakingPoolState.rewardTokenAccount.publicKey,
            stakingProgramAuthority: stakingProgramAuthority,
          },
          clock: SYSVAR_CLOCK_PUBKEY,
          rent: SYSVAR_RENT_PUBKEY,
        }
      };
      const claimAndSellIx = pole.instruction.claimAndSell(
        claimAndSellAccs
      );
      const tx = new Transaction();
      tx.add(
        claimAndSellIx
      );

      await pole.provider.send(tx);

      const afterLiquidityAmount = new anchor.BN((await provider.connection.getTokenAccountBalance(freshPolePool.genericConfig.liquiditySupply)).value.amount);
      const liquidityGained = afterLiquidityAmount.sub(beforeLiquidityAmount);
      if(!liquidityGained.gt(new anchor.BN(0))) console.log("Should get some liquidity");
  };


  const withdraw = (amount: number | string | anchor.BN) => async () => {
    const [polePoolAddr] = await PublicKey.findProgramAddress(
      [Uint8Array.from(poolName.split("").map(c => c.charCodeAt(0)))],
      pole.programId
    );
    const freshPolePool = await fetchPolePool(pole, polePoolAddr);
    const [poleAuthority] = await PublicKey.findProgramAddress(
      [],
      pole.programId
    );

    const poleLpTokenAddress = await Token.getAssociatedTokenAddress(
      ASSOCIATED_TOKEN_PROGRAM_ID,
      TOKEN_PROGRAM_ID,
      freshPolePool.genericConfig.lpMint,
      provider.wallet.publicKey,
    );
    if (amount == "ALL") {
      const poleLpTokenAmount = new anchor.BN(
        (await pole.provider.connection.getTokenAccountBalance(poleLpTokenAddress)).value.amount
      );
      amount = poleLpTokenAmount
    }
    // console.log("withdraw amount", amount.toString())
    const reserve = await fetchReserve(provider, reserveState.address);

    const withdrawAccs = {
      accounts: {
        polePool: polePoolAddr,
        poleAuthority: poleAuthority,
        poleLiquidityAccounts: {
          userLiquidityWallet: serumOrderBook.usdcTokenAccount,
          poleLiquidityWallet: freshPolePool.genericConfig.liquiditySupply
        },
        poleLpAccounts: {
          lpMint: freshPolePool.genericConfig.lpMint,
          userLpWallet: poleLpTokenAddress
        },
        portAccounts: await generatePortAccounts(
          serumOrderBook.usdcMint, reserveState.address, reserve, freshPolePool),
        userTransferAuthority: provider.wallet.publicKey,
        tokenProgram: freshPolePool.genericConfig.tokenProgram,
        clock: SYSVAR_CLOCK_PUBKEY,
        poleFeeAccount: freshPolePool.genericConfig.feeReceiver
      },
      signers: []
    };

    const verifyIx = pole.instruction.verifyRedeem(
      {
        accounts: {
          polePool: polePoolAddr,
          transactionInfo: SYSVAR_INSTRUCTIONS_PUBKEY
        }
      }
    );
    const withdrawIx = pole.instruction.redeemLiquidity(
      new anchor.BN(amount),
      withdrawAccs
    );

    const withdrawIx2 = pole.instruction.redeemLiquidity(
      new anchor.BN(0),
      withdrawAccs
    );

    const refreshReserveIx = refreshReserveInstruction(
      reserveState.address,
      null
    );
    const refreshObligationIx = refreshObligationInstruction(
      poleState.obligation,
      [reserveState.address],
      [reserveState.address]
    );

    const withdrawTx = new Transaction();
    withdrawTx.add(
      verifyIx,
      refreshReserveIx,
      refreshObligationIx,
      withdrawIx,
    );

    for (let i = 1; i < freshPolePool.portConfig.portIterate; i += 1) {
      withdrawTx.add(
        refreshReserveIx,
        refreshObligationIx,
        withdrawIx2,
      )
    }
    const lp_amount = freshPolePool.basicState.lpAmount;
    const lp_percentage = new Big(amount.toString()).div(new Big(lp_amount.toString()));

    const beforePoleLiquidity = new Big((await provider.connection.getTokenAccountBalance(freshPolePool.genericConfig.liquiditySupply)).value.amount);
    const parsedReserve = await fetchReserve(provider, reserveState.address);
    const beforeTotalLiquidity = new Big(parsedReserve.liquidity.availableAmount.toString()).add(beforePoleLiquidity);
    const liquidityShouldGet = beforeTotalLiquidity.mul(lp_percentage).mul(new Big("10000").sub(new Big(freshPolePool.genericConfig.withdrawFeeBips.toString()))).div(new Big("10000"));
    const beforeUserLiquidity = new Big( (await provider.connection.getTokenAccountBalance(serumOrderBook.usdcTokenAccount)).value.amount);
    await provider.send(withdrawTx);
    const fee_received =  new anchor.BN((await provider.connection.getTokenAccountBalance(freshPolePool.genericConfig.feeReceiver)).value.amount);
    assert(fee_received > new anchor.BN(0));

    const afterUserLiquidity = new Big((await provider.connection.getTokenAccountBalance(serumOrderBook.usdcTokenAccount)).value.amount);
    const liquidityGet = afterUserLiquidity.sub(beforeUserLiquidity);

    assert(liquidityGet.lte(liquidityShouldGet))
    assert(liquidityGet.gte(liquidityShouldGet.mul(0.999)) || liquidityGet.eq(liquidityShouldGet.toFixed(0, 0)), liquidityGet.toString() + ", " + liquidityShouldGet.toString())
  };

  const sleepTest = (ms: number) => it('Sleep', async () => {
    await sleep(ms)
  });
  it('Set up pole', async () => {
    poleState = await createPolePool(pole, lendingMarket.publicKey, serumOrderBook, stakingPoolState, reserveState);
  });
  it('Be able to deposit into Pole successfully',deposit(5_000_000_000_000_00, false));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to withdraw', withdraw(19_000_000_000_000));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to deposit into Pole successfully',deposit(100_000, true));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to deposit into Pole successfully',deposit(20_000_000_000_000, true));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to deposit into Pole successfully',deposit(10_000, true));
  sleepTest(10_000);
  it('Be able to claim and sell',claim_and_sell);
  it('Claim and withdraw (no unrolling)', withdraw(10_000));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to deposit into Pole successfully', deposit(100, true));
  it('Be able to claim and sell',claim_and_sell);
  it('Claim and withdraw (no unrolling)', withdraw(150));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to deposit into Pole successfully', deposit(100_000_000, true));
  it('Be able to claim and sell',claim_and_sell);
  it('Be able to deposit into Pole successfully', deposit(1000_000_000, true));
  it('Be able to Change liquidity cap', change_liquidity_cap(1_000_000));
  //would fail it('Be able to deposit into Pole successfully', deposit(1_500_000, true));
});
