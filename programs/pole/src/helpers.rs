use anchor_lang::prelude::*;
use anchor_spl::token;
use port_variable_rate_lending_instructions::state::CollateralExchangeRate;

use crate::error::PoleError;
use crate::{PolePortAccounts, PortLendingAccounts, PortLendingLeveragingParams};
use port_anchor_adaptor::port_accessor::{
    exchange_rate, obligation_borrows_count, obligation_deposits_count, obligation_liquidity,
};
use port_anchor_adaptor::{borrow, deposit_and_collateralize, repay, withdraw};
use port_anchor_adaptor::{redeem as port_redeem, refresh_port_obligation, refresh_port_reserve};
use solana_maths::{Decimal, Rate, TryMul};

#[inline(always)]
pub fn port_lending_leveraging<'info, 'a>(
    port_accounts: &PortLendingAccounts<'info>,
    params: &PortLendingLeveragingParams<'info, 'a>,
    borrow_fraction: u64,
    iterate: u8,
    current_leverage: u8, // [0..iterate)
    bump: u8,
) -> ProgramResult {
    let available_liquidity = token::accessor::amount(params.user_liquidity)?;
    //assume reserve and obligation are refreshed
    deposit_and_collateralize(
        port_accounts.create_deposit_and_collateralize_context(params, &[&[&[bump]]]),
        available_liquidity,
    )?;
    if current_leverage < iterate.checked_sub(1).ok_or(PoleError::MathOverflow)? {
        refresh_port_reserve_and_obligation(port_accounts, params)?;
        borrow(
            port_accounts.create_borrow_context(params, &[&[&[bump]]]),
            available_liquidity * borrow_fraction / 100,
        )?;
    }
    Ok(())
}
#[inline(always)]
pub fn port_lending_unroll<'info, 'a>(
    port_accounts: &PortLendingAccounts<'info>,
    params: &PortLendingLeveragingParams<'info, 'a>,
    repay_ratio: Decimal,
    iterate: u8,
    current_leverage: u8, // [0..iterate)
    bump: u8,
    mut unroll_amount: u64,
) -> ProgramResult {
    //assume reserve and obligation are refreshed
    let port_exchange_rate = exchange_rate(&port_accounts.reserve)?;

    let mut repay_amount = 0;
    let mut withdraw_amount = 0;

    for _ in 0..iterate
        .checked_sub(current_leverage)
        .ok_or(PoleError::MathOverflow)?
    {
        withdraw_amount = port_exchange_rate
            .liquidity_to_collateral(unroll_amount)?
            .checked_add(1)
            .ok_or(PoleError::MathOverflow)?;
        let available_liquidity_to_get = port_exchange_rate
            .collateral_to_liquidity(withdraw_amount)?
            .checked_add(1)
            .ok_or(PoleError::MathOverflow)?;
        // msg!("liquidity to get {:?}", available_liquidity_to_get);
        repay_amount = repay_ratio
            .try_mul(available_liquidity_to_get)?
            .try_ceil_u64()?;
        unroll_amount = repay_amount;
    } //Proof by induction

    if current_leverage == 0 {
        withdraw(
            port_accounts.create_withdraw_context(params, &[&[&[bump]]]),
            withdraw_amount,
        )?;
        port_redeem(
            port_accounts.create_redeem_context(params, &[&[&[bump]]]),
            withdraw_amount,
        )?;
    } else {
        repay(
            port_accounts.create_repay_context(params, &[&[&[bump]]]),
            repay_amount,
        )?;
        refresh_port_reserve_and_obligation(port_accounts, params)?;

        withdraw(
            port_accounts.create_withdraw_context(params, &[&[&[bump]]]),
            withdraw_amount,
        )?;
        port_redeem(
            port_accounts.create_redeem_context(params, &[&[&[bump]]]),
            withdraw_amount,
        )?;
    }
    Ok(())
}

#[inline(always)]
pub fn refresh_port_reserve_and_obligation<'info>(
    port_accounts: &PortLendingAccounts<'info>,
    params: &PortLendingLeveragingParams<'info, '_>,
) -> ProgramResult {
    let obligation = &port_accounts.obligation;
    refresh_port_reserve(port_accounts.create_fresh_reserve_context(params, vec![]))?;
    if obligation_deposits_count(obligation)? == 0 && obligation_borrows_count(obligation)? == 0 {
        Ok(())
    } else if obligation_deposits_count(obligation)? == 1
        && obligation_borrows_count(obligation)? == 0
    {
        refresh_port_obligation(
            port_accounts.create_refresh_obligation_context(params, vec![&port_accounts.reserve]),
        )
    } else {
        refresh_port_obligation(port_accounts.create_refresh_obligation_context(
            params,
            vec![&port_accounts.reserve, &port_accounts.reserve],
        ))
    }
}
macro_rules! assert_state {
    ($e:expr, $s:literal) => {
        if !$e {
            msg!("State Error: {}", $s);
            Err(PoleError::InvalidPoolState)
        } else {
            Ok(())
        }
    };
}
pub fn valid_pole_pool<T: PolePortAccounts>(ctx: &Context<T>) -> ProgramResult {
    let pole_pool = ctx.accounts.get_pole_pool()?;
    assert_state!(pole_pool.generic_config.validate(), "Generic config")?;
    assert_state!(pole_pool.port_config.validate(), "Port config")?;
    assert_state!(
        pole_pool.port_config.port_iterate > pole_pool.port_state.leverage,
        "Leverage sanity check"
    )?;
    assert_state!(
        pole_pool.port_state.deposit_verified == 0 || pole_pool.port_state.deposit_verified == 1,
        "Deposit verify sanity check"
    )?;
    assert_state!(
        pole_pool.port_state.redeem_verified == 0 || pole_pool.port_state.redeem_verified == 1,
        "Redeem verify sanity check"
    )?;
    assert_state!(
        !(pole_pool.port_state.redeem_verified == 1 && pole_pool.port_state.deposit_verified == 1),
        "Only one of deposit and redeem can be enabled"
    )?;

    assert_state!(
        pole_pool.port_state.is_redeemed == 0 || pole_pool.port_state.is_redeemed == 1,
        "Redeem verify sanity check"
    )?;
    let init_liquidity = std::ptr::addr_of!(pole_pool.port_state.init_port_liquidity);
    let init_user_liquidity_percentage =
        std::ptr::addr_of!(pole_pool.port_state.user_liquidity_percentage);
    assert_state!(
        if pole_pool.port_state.leverage == 0 {
            (unsafe { *init_liquidity == Decimal::zero().0 .0 })
                && (unsafe { *init_user_liquidity_percentage == Rate::zero().0 .0 })
                && (pole_pool.port_state.amount_to_unroll == 0)
                && (pole_pool.port_state.redeem_amount == 0)
        } else if pole_pool.basic_state.lp_amount != 0 {
            unsafe { *init_liquidity != Decimal::zero().0 .0 }
        } else {
            true
        },
        "Init liquidity"
    )?;
    Ok(())
}

#[inline(always)]
pub fn get_port_liquidity<'info>(
    exchange_rate: &CollateralExchangeRate,
    obligation: &AccountInfo<'info>,
) -> Result<Decimal, ProgramError> {
    obligation_liquidity(obligation, exchange_rate, 0, 0)
}
