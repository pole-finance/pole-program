import { PublicKey } from "@solana/web3.js";
import BigNumber from "bignumber.js";

export const TOKEN_ACCOUNT_LEN = 165;
export const TOKEN_MINT_LEN = 82;
export const RESERVE_LEN = 575;
export const LENDING_MARKET_LEN = 258;
export const STAKING_POOL_LEN = 298;
export const WAD = new BigNumber('1000000000000000000');
export const PORT_STAKING = new PublicKey("stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq");
export const PORT_LENDING = new PublicKey("Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR");
export const SERUM_DEX_PROGRAM_ID = new PublicKey("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin");
export const SWAP_PROGRAM_ID = new PublicKey("22Y43yTVxuUkoRKdm9thyRhQ3SdgQS7c7kB6UNCiaczD");