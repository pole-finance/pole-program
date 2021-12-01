use anchor_lang::prelude::*;

#[event]
pub struct DidDeposit {
    pub liquidity_amount_deposited: u64,
    pub lp_amount_minted: u64,
}

#[event]
pub struct DidRedeem {
    pub liquidity_amount_redeemed: u64,
    pub lp_amount_burned: u64,
}

#[event]
pub struct DidSell {
    pub base_amount: u64,
    pub slot: u64,
}

#[event]
pub struct DidStake {
    pub amount: u64,
    pub slot: u64,
}

#[event]
pub struct DidUnstake {
    pub amount: u64,
    pub slot: u64,
}
