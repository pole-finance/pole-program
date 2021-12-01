import { BN, Program, Provider } from "@project-serum/anchor";
import { Market } from "@project-serum/serum";
import { TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { Keypair, PublicKey, SystemProgram, SYSVAR_CLOCK_PUBKEY, SYSVAR_RENT_PUBKEY, Transaction } from "@solana/web3.js"
import { Pole } from "../../target/types/pole";
import { PORT_LENDING, PORT_STAKING, SERUM_DEX_PROGRAM_ID, SWAP_PROGRAM_ID } from "../constants";
import { ReserveState } from "../port";
import { parseReserve, Reserve } from "../state/reserve";

export const createAccount = async (provider: Provider, space: number, owner: PublicKey): Promise<Keypair> => {
  const newAccount = Keypair.generate();
  const createTx = new Transaction().add(
      SystemProgram.createAccount({
          fromPubkey: provider.wallet.publicKey,
          newAccountPubkey: newAccount.publicKey,
          programId: owner,
          lamports: await provider.connection.getMinimumBalanceForRentExemption(
              space
          ),
          space,
      })
  );
  await provider.send(
    createTx,
    [newAccount]
  );
  return newAccount
}

// eslint-disable-next-line
export const fetchPolePool = async (program: Program<Pole>, address: PublicKey): Promise<any> => {
  const polePoolRaw = await program.provider.connection.getAccountInfo(address);
  if (!polePoolRaw || !polePoolRaw.data) {
    throw new Error("pool data not found.");
  }
  const polePoolData = await program.coder.accounts.decode('polePortPool', polePoolRaw?.data);
  return polePoolData;
}

export const fetchReserve = async (provider: Provider, address: PublicKey): Promise<Reserve> => {
  const reserveRaw = await provider.connection.getAccountInfo(address);
  if (!reserveRaw || !reserveRaw.data) {
    throw new Error("pool data not found.");
  }
  const parsedReserve = parseReserve(address, reserveRaw);

  if (!parsedReserve) {
    throw new Error("failed parsing reserve");
  }

  return parsedReserve.data
}

export interface SerumState {
  marketPortUSDC: Market;
  marketMaker: { tokens: Map<string, PublicKey>; account: Keypair; };
  portMint: PublicKey;
  usdcMint: PublicKey;
  portTokenAccount: PublicKey;
  usdcTokenAccount: PublicKey;
}

export interface StakingPoolState {
  stakingPool: Keypair;
  rewardTokenAccount: Keypair;
}

export interface PoleState {
  lpMint: PublicKey;
  feeReceiver: PublicKey;
  openOrders: PublicKey;
  obligation: PublicKey;
  stakeAccount: PublicKey;
}

export const createPolePool = async (
  pole: Program<Pole>, lendingMarket: PublicKey, serumOrderBook: SerumState, stakingPoolState: StakingPoolState, reserveState: ReserveState): Promise<PoleState> => {
  const poolName = "USDC";
  const lpMint = Keypair.generate();
  const feeReceiver = Keypair.generate();
  const openOrders = Keypair.generate();
  const obligation = Keypair.generate();
  const stakeAccount = Keypair.generate();
  const [poleAuthority, authority_bump] = await PublicKey.findProgramAddress(
    [],
    pole.programId
  );
  const [polePool, address_bump] = await PublicKey.findProgramAddress(
    [Uint8Array.from(poolName.split("").map(c => c.charCodeAt(0)))],
    pole.programId
  );
  
  const liquiditySupply = await Keypair.generate();
  const portLpSupply = await Keypair.generate();
  const portSupply = await Keypair.generate();
  await pole.rpc.createPool(
    authority_bump,
    poolName,
    address_bump,
    {
      liquidityCap: new BN(1000000000000000),
      withdrawFeeBips: 10,
      portIterate: 5,
      portReservePercentage: 5,
      portMinDeposit: new BN(100),
      swapProgram: SWAP_PROGRAM_ID,
      reserve: reserveState.address
    },
    {
      accounts: {
        polePool: polePool,
        lpMint: lpMint.publicKey,
        liquiditySupply: liquiditySupply.publicKey,
        portLpSupply: portLpSupply.publicKey,
        portSupply: portSupply.publicKey,
        feeReceiver: feeReceiver.publicKey,
        portOpenOrders: openOrders.publicKey,
        liquidityMint: serumOrderBook.usdcMint,
        portLpMint: reserveState.collateralMintAccount,
        portMint: serumOrderBook.portMint,
        poleAuthority: poleAuthority,
        user: pole.provider.wallet.publicKey,
        dexMarket: serumOrderBook.marketPortUSDC.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        dexProgram: SERUM_DEX_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      },
      signers: [lpMint,
        feeReceiver, openOrders,liquiditySupply,portLpSupply,portSupply
      ],
    }
  );

  await pole.rpc.initPortAccounts(
    {
      accounts: {
        polePool: polePool,
        obligation: obligation.publicKey,
        stakeAccount: stakeAccount.publicKey,
        poleAuthority: poleAuthority,
        user: pole.provider.wallet.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
        portLendingProgram: PORT_LENDING,
        portStakingProgram: PORT_STAKING,
        portLendingMarket: lendingMarket,
        stakingPool: stakingPoolState.stakingPool.publicKey,
        rent: SYSVAR_RENT_PUBKEY,
        clock: SYSVAR_CLOCK_PUBKEY,
        systemProgram: SystemProgram.programId,
      },
      signers: [
        obligation,
        stakeAccount
      ]
    },
  );
  return {
    lpMint: lpMint.publicKey,
    feeReceiver: feeReceiver.publicKey,
    openOrders: openOrders.publicKey,
    obligation: obligation.publicKey,
    stakeAccount: stakeAccount.publicKey
  }
}
