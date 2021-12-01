// Migrations are an early feature. Currently, they're nothing more than this
// single deploy script that's invoked from the CLI, injecting a provider
// configured from the workspace's Anchor.toml.
import * as anchor from "@project-serum/anchor";
import { setupAMarket } from "../tests/serum/index";
import {
  createDefaultReserve,
  createLendingMarket,
  createStakingPool,
} from "../tests/port";
import { PublicKey } from "@solana/web3.js";
import { PORT_LENDING } from "../tests/constants";
import { Program } from "@project-serum/anchor";
import { Pole, IDL } from "../target/types/pole";
import { createPolePool } from "../tests/utils";
import * as fs from "fs";
import * as path from "path";

module.exports = async function (provider: anchor.Provider) {
  // Configure client to use the provider.
  anchor.setProvider(provider);
  console.log("Provider public key: ", provider.wallet.publicKey.toString());

  // Add your deploy script here.
  const serumOrderBook = await setupAMarket({ provider });
  const lendingMarket = await createLendingMarket(provider);
  const [lendingMarketAuthority] = await PublicKey.findProgramAddress(
    [lendingMarket.publicKey.toBuffer()],
    PORT_LENDING
  );
  const stakingPool = await createStakingPool(
    provider,
    serumOrderBook.portTokenAccount,
    serumOrderBook.portMint,
    lendingMarketAuthority,
    100000000
  );
  const reserveState = await createDefaultReserve(
    provider,
    1,
    serumOrderBook.usdcTokenAccount,
    lendingMarket.publicKey,
    stakingPool.stakingPool.publicKey
  );
  const pole = new Program<Pole>(
    IDL,
    anchor.workspace.Pole.programId,
    provider
  );
  await createPolePool(
    pole,
    lendingMarket.publicKey,
    serumOrderBook,
    stakingPool,
    reserveState
  );
  console.log(
    `
lendingMarket: ${lendingMarket.publicKey.toString()}
serumMarket for PORT: ${serumOrderBook.marketPortUSDC.address.toString()}
reserveState: ${reserveState.address.toString()}
stakingPool: ${stakingPool.stakingPool.publicKey.toString()}
fake USDC Mint: ${serumOrderBook.usdcMint.toString()}
fake PORT Mint: ${serumOrderBook.portMint.toString()}

`
  );
  const outDir = path.resolve(`${__dirname}`, "../app/src/local.ts");
  fs.writeFileSync(
    outDir,
    `import { PublicKey } from "@solana/web3.js";

export const PORT_MARKET_LOCAL_ADDRESS = new PublicKey(
  "${serumOrderBook.marketPortUSDC.address.toString()}"
);
export const PORT_LOCAL_MINT = new PublicKey(
  "${serumOrderBook.portMint.toString()}"
);
export const RESERVE_LOCAL = new PublicKey(
  "${reserveState.address.toString()}"
);
`
  );
};
