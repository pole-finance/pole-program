use anchor_lang::prelude::*;

use crate::StakingBumps;
use solana_maths::{Decimal, Rate, TryDiv, TryMul};

pub(crate) const DISCRIMINATOR_SIZE: usize = 8;
// 64*4;
#[account(zero_copy)]
#[derive(Debug, PartialEq)]
pub struct PolePortPool {
    pub generic_config: GenericPoolConfig,
    pub port_config: PortConfig,
    pub serum_config: SerumConfig,
    pub basic_state: BasicState,
    pub port_state: PortState,
    pub _padding: [u64; 30],
}

#[account]
#[derive(Debug, PartialEq)]
pub struct StakingPool {
    pub bumps: StakingBumps,
    pub yielding_pool: Pubkey,
    pub port_staking_pool: Pubkey,
    pub lp_wallet: Pubkey,
    pub owner: Pubkey,
    pub staking_program: Pubkey,
    pub token_program: Pubkey,
}

#[zero_copy]
#[derive(Debug, PartialEq)]
pub struct GenericPoolConfig {
    pub bump: u64, //u8
    pub liquidity_cap: u64,
    pub withdraw_fee_bips: u64, //u8
    pub fee_receiver: Pubkey,
    pub owner: Pubkey,
    pub lp_mint: Pubkey,
    pub liquidity_supply: Pubkey,
    pub token_program: Pubkey,
}
impl GenericPoolConfig {
    pub fn validate(&self) -> bool {
        self.bump <= 255 && self.withdraw_fee_bips <= 255
    }
}
#[zero_copy]
#[derive(Debug, PartialEq)]
pub struct PortConfig {
    pub min_deposit: u64, // > 10
    pub obligation: Pubkey,
    pub reserve: Pubkey,
    pub stake_account: Pubkey,
    pub port_lending_program: Pubkey,
    pub port_staking_program: Pubkey,
    pub port_token_mint: Pubkey,
    pub port_lp_supply: Pubkey,
    pub port_supply: Pubkey,
    pub port_iterate: u64,            // < 10
    pub port_reserve_percentage: u64, // < 10
}
impl PortConfig {
    pub fn validate(&self) -> bool {
        self.min_deposit > 10 && self.port_iterate < 10 && self.port_reserve_percentage < 10
    }
}
#[zero_copy]
#[derive(Debug, PartialEq)]
pub struct SerumConfig {
    pub dex_program: Pubkey,
    pub swap_program: Pubkey,
    pub port_open_orders: Pubkey,
}

#[zero_copy]
#[derive(Debug, PartialEq, Default)]
pub struct BasicState {
    pub lp_amount: u64,
}

#[zero_copy]
#[derive(Debug, PartialEq, Default)]
pub struct PortState {
    pub deposit_verified: u64,         //boolean
    pub redeem_verified: u64,          //boolean
    pub leverage: u64,                 // <= port_iterate
    pub init_port_liquidity: [u64; 3], //Decimal
    pub approved_wallet: Pubkey,
    pub is_redeemed: u64,
    pub redeem_amount: u64,
    pub amount_to_unroll: u64,
    pub last_sold_slot: u64,
    pub user_liquidity_percentage: [u64; 2], // Rate
}

impl PortState {
    pub fn reset(&mut self) {
        self.deposit_verified = 0;
        self.redeem_verified = 0;
        self.leverage = 0;
        self.init_port_liquidity = Decimal::zero().0 .0;
        self.approved_wallet = Pubkey::default();
        self.is_redeemed = 0;
        self.redeem_amount = 0;
        self.amount_to_unroll = 0;
        self.user_liquidity_percentage = Rate::zero().0 .0;
    }
}

impl BasicState {
    pub fn exchange_rate(&self, liquidity_amount: Decimal) -> Result<ExchangeRate, ProgramError> {
        if liquidity_amount == Decimal::zero() || self.lp_amount == 0 {
            Ok(ExchangeRate(Decimal::one()))
        } else {
            Ok(ExchangeRate(
                Decimal::from(self.lp_amount).try_div(liquidity_amount)?,
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Copy)]
pub struct ExchangeRate(Decimal);

impl ExchangeRate {
    pub fn liquidity_to_lp(&self, liquidity_amount: u64) -> Result<u64, ProgramError> {
        self.0.try_mul(liquidity_amount)?.try_floor_u64()
    }
    pub fn lp_to_liquidity(&self, lp_amount: u64) -> Result<u64, ProgramError> {
        Decimal::from(lp_amount).try_div(self.0)?.try_floor_u64()
    }
}
#[account]
#[derive(Debug, PartialEq)]
pub struct UserBalance {
    pub balance: u64,
    pub last_update: u64,
    pub user: Pubkey,
    pub _padding: [u64; 16],
}
