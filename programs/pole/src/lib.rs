use anchor_lang::prelude::*;

use crate::instructions::*;

pub mod adaptors;
pub mod error;
pub mod event;
pub mod helpers;
pub mod instructions;
pub mod states;

declare_id!("PoLEr5uRhLSpEZgmBaSmzTUVbEANuFp4vBARZbKsqnu");
const DEPOSIT_LIQUIDITY_SIGHASH: [u8; 8] = [245, 99, 59, 25, 151, 71, 233, 249]; //update this when update the instruction name
const REDEEM_LIQUIDITY_SIGHASH: [u8; 8] = [180, 117, 142, 137, 227, 225, 97, 211]; //update this when update the instruction name
#[program]
pub mod pole {
    use anchor_spl::dex::init_open_orders;
    use anchor_spl::token;
    use anchor_spl::token::{burn, transfer, Transfer};
    use serum_swap::{ExchangeRate, Side};
    use std::convert::TryFrom;

    use crate::adaptors::decimal;
    use crate::error::PoleError;
    use crate::event::{DidDeposit, DidRedeem, DidSell};
    use crate::helpers::*;
    use crate::states::{BasicState, GenericPoolConfig, PortConfig, PortState, SerumConfig};
    use anchor_lang::solana_program::sysvar::instructions::get_instruction_relative;
    use anchor_spl::dex::serum_dex::state::MarketState;
    use anchor_spl::token::accessor::amount;
    use port_anchor_adaptor::port_accessor::exchange_rate;
    use port_anchor_adaptor::*;
    use solana_maths::{Decimal, Rate, TryAdd, TryDiv, TryMul, TrySub, U128, U192};

    use super::*;

    pub fn create_pool(
        ctx: Context<CreatePool>,
        authority_bump: u8,
        _name: String,
        _pda_bump: u8,
        init_params: InitParams,
    ) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_init()?;
        pole_pool.generic_config = GenericPoolConfig {
            liquidity_cap: init_params.liquidity_cap,
            withdraw_fee_bips: init_params.withdraw_fee_bips as u64,
            fee_receiver: ctx.accounts.fee_receiver.key(),
            owner: ctx.accounts.user.key(),
            lp_mint: ctx.accounts.lp_mint.key(),
            liquidity_supply: ctx.accounts.liquidity_supply.key(),
            token_program: ctx.accounts.token_program.key(),
            bump: authority_bump as u64,
        };

        let port_config = PortConfig {
            port_iterate: init_params.port_iterate as u64,
            port_reserve_percentage: init_params.port_reserve_percentage as u64,
            min_deposit: init_params.port_min_deposit,
            reserve: init_params.reserve,
            obligation: Pubkey::default(),           //not init yet
            stake_account: Pubkey::default(),        //not init yet
            port_lending_program: Pubkey::default(), //not init yet
            port_staking_program: Pubkey::default(), //not init yet
            port_token_mint: ctx.accounts.port_mint.key(),
            port_lp_supply: ctx.accounts.port_lp_supply.key(),
            port_supply: ctx.accounts.port_supply.key(),
        };
        let serum_config = SerumConfig {
            dex_program: ctx.accounts.dex_program.key(),
            swap_program: init_params.swap_program,
            port_open_orders: ctx.accounts.port_open_orders.key(),
        };
        pole_pool.port_config = port_config;
        pole_pool.serum_config = serum_config;
        pole_pool.basic_state = BasicState::default();
        pole_pool.port_state = PortState::default();

        init_open_orders(ctx.accounts.create_init_open_orders_cpi(
            ctx.accounts.port_open_orders.clone(),
            &[&[&[pole_pool.generic_config.bump as u8]]],
        ))?;

        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn init_port_accounts(ctx: Context<InitPortAccounts>) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;

        let port_config = &mut pole_pool.port_config;
        port_config.obligation = ctx.accounts.obligation.key();
        port_config.stake_account = ctx.accounts.stake_account.key();
        port_config.port_lending_program = ctx.accounts.port_lending_program.key();
        port_config.port_staking_program = ctx.accounts.port_staking_program.key();
        init_obligation(
            ctx.accounts
                .create_init_obligation_cpi(&[&[&[pole_pool.generic_config.bump as u8]]]),
        )?;

        create_stake_account(
            ctx.accounts
                .create_create_stake_account_cpi(&[&[&[pole_pool.generic_config.bump as u8]]]),
        )?;
        Ok(())
    }

    pub fn verify_deposit(ctx: Context<VerifyDeposit>) -> ProgramResult {
        let transaction = &ctx.accounts.transaction_info;
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        let mut count = 0;
        let mut approved_wallet = Pubkey::default();
        let mut i = 1;
        while let Ok(ins) = get_instruction_relative(i as i64, transaction) {
            if ins.program_id == pole::id() {
                let mut ix_data: &[u8] = &ins.data;
                let sighash: [u8; 8] = {
                    let mut sighash: [u8; 8] = [0; 8];
                    sighash.copy_from_slice(&ix_data[..8]);
                    ix_data = &ix_data[8..];
                    sighash
                };
                if sighash != DEPOSIT_LIQUIDITY_SIGHASH {
                    return Err(PoleError::InvalidTransaction.into());
                } else {
                    let ix = crate::instruction::DepositLiquidity::deserialize(&mut &*ix_data)
                        .map_err(|_| {
                            anchor_lang::__private::ErrorCode::InstructionDidNotDeserialize
                        })?;
                    let crate::instruction::DepositLiquidity { amount } = ix;
                    if (amount > 0) ^ (count == 0) {
                        return Err(PoleError::DepositAmountInvalid.into());
                    }
                    let wallet = ins.accounts[2].pubkey;
                    if approved_wallet == Pubkey::default() {
                        approved_wallet = wallet;
                    } else if approved_wallet != wallet {
                        return Err(PoleError::WrongWallet.into());
                    }
                    count += 1;
                }
            }
            if count == pole_pool.port_config.port_iterate {
                break;
            }
            i += 1;
        }
        if count != pole_pool.port_config.port_iterate {
            return Err(PoleError::InvalidTransaction.into());
        }

        pole_pool.port_state.deposit_verified = 1;
        pole_pool.port_state.approved_wallet = approved_wallet;
        Ok(())
    }

    pub fn verify_redeem(ctx: Context<VerifyRedeem>) -> ProgramResult {
        let transaction = &ctx.accounts.transaction_info;
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        let mut count = 0;
        let mut approved_wallet = Pubkey::default();
        let mut redeem_amount = 0;
        let mut i = 1;
        while let Ok(ins) = get_instruction_relative(i as i64, transaction) {
            if ins.program_id == pole::id() {
                let mut ix_data: &[u8] = &ins.data;
                let sighash: [u8; 8] = {
                    let mut sighash: [u8; 8] = [0; 8];
                    sighash.copy_from_slice(&ix_data[..8]);
                    ix_data = &ix_data[8..];
                    sighash
                };
                if sighash != REDEEM_LIQUIDITY_SIGHASH {
                    return Err(PoleError::InvalidTransaction.into());
                } else {
                    let ix = crate::instruction::RedeemLiquidity::deserialize(&mut &*ix_data)
                        .map_err(|_| {
                            anchor_lang::__private::ErrorCode::InstructionDidNotDeserialize
                        })?;
                    let crate::instruction::RedeemLiquidity { amount } = ix;

                    if redeem_amount == 0 {
                        redeem_amount = amount;
                    }

                    if (amount > 0) ^ (count == 0) {
                        return Err(PoleError::RedeemAmountInvalid.into());
                    }
                    let wallet = ins.accounts[6].pubkey;
                    if approved_wallet == Pubkey::default() {
                        approved_wallet = wallet;
                    } else if approved_wallet != wallet {
                        return Err(PoleError::InvalidTransaction.into());
                    }
                    count += 1;
                }
            }
            if count == pole_pool.port_config.port_iterate {
                break;
            }
            i += 1;
        }
        if count != pole_pool.port_config.port_iterate {
            return Err(PoleError::InvalidTransaction.into());
        }

        pole_pool.port_state.redeem_verified = 1;
        pole_pool.port_state.approved_wallet = approved_wallet;
        Ok(())
    }
    //assume reserve and obligation are refreshed
    #[access_control(valid_pole_pool(&ctx))]
    pub fn deposit_liquidity(ctx: Context<DepositLiquidity>, amount: u64) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        if pole_pool.port_state.leverage == 0 {
            let port_exchange_rate = exchange_rate(&ctx.accounts.port_accounts.reserve)?;

            let port_liquidity =
                get_port_liquidity(&port_exchange_rate, &ctx.accounts.port_accounts.obligation)?;

            let pole_init_liquidity = token::accessor::amount(
                &ctx.accounts.pole_liquidity_accounts.pole_liquidity_wallet,
            )?;

            if port_liquidity
                .try_add(pole_init_liquidity.into())?
                .try_add(amount.into())?
                .ge(&pole_pool.generic_config.liquidity_cap.into())
            {
                return Err(PoleError::MeetDepositLimit.into());
            }

            let user_liquidity_percentage =
                Decimal::from(amount).try_div(amount + pole_init_liquidity)?;

            token::transfer(
                ctx.accounts
                    .pole_liquidity_accounts
                    .create_transfer_user_to_pole_context(
                        ctx.accounts.user_transfer_authority.to_account_info(),
                        ctx.accounts.token_program.clone(),
                        &[&[&[pole_pool.generic_config.bump as u8]]],
                    ),
                amount,
            )?;
            pole_pool.port_state.init_port_liquidity = port_liquidity.0 .0;

            pole_pool.port_state.user_liquidity_percentage =
                Rate::try_from(user_liquidity_percentage)?.0 .0;
        }

        let ltv = port_accessor::reserve_ltv(&ctx.accounts.port_accounts.reserve)?;
        let lending_leveraging_params = PortLendingLeveragingParams {
            user_liquidity: &ctx.accounts.pole_liquidity_accounts.pole_liquidity_wallet,
            obligation_owner: &ctx.accounts.pole_authority,
            transfer_authority: &ctx.accounts.pole_authority,
            clock: &ctx.accounts.clock.to_account_info(),
            token_program: &ctx.accounts.token_program,
        };

        port_lending_leveraging(
            &ctx.accounts.port_accounts,
            &lending_leveraging_params,
            (ltv as u64)
                .checked_sub(pole_pool.port_config.port_reserve_percentage)
                .ok_or(PoleError::MathOverflow)?,
            pole_pool.port_config.port_iterate as u8,
            pole_pool.port_state.leverage as u8,
            pole_pool.generic_config.bump as u8,
        )?;

        pole_pool.port_state.leverage = pole_pool
            .port_state
            .leverage
            .checked_add(1)
            .ok_or(PoleError::MathOverflow)?;
        if pole_pool.port_state.leverage == pole_pool.port_config.port_iterate {
            let port_exchange_rate = exchange_rate(&ctx.accounts.port_accounts.reserve)?;
            let init_port_liquidity = Decimal(U192(pole_pool.port_state.init_port_liquidity));

            let after_port_liquidity =
                get_port_liquidity(&port_exchange_rate, &ctx.accounts.port_accounts.obligation)?;

            assert_eq!(
                token::accessor::amount(
                    &ctx.accounts.pole_liquidity_accounts.pole_liquidity_wallet
                )?,
                0u64
            );

            let user_liquidity_rate = Rate(U128(pole_pool.port_state.user_liquidity_percentage));

            let user_liquidity_gain = after_port_liquidity
                .try_sub(init_port_liquidity)?
                .try_mul(user_liquidity_rate)?
                .try_floor_u64()?;
            let exchange_rate = pole_pool
                .basic_state
                .exchange_rate(after_port_liquidity.try_sub(user_liquidity_gain.into())?)?;

            let mint_amount = exchange_rate.liquidity_to_lp(user_liquidity_gain)?;
            pole_pool.basic_state.lp_amount = pole_pool
                .basic_state
                .lp_amount
                .checked_add(mint_amount)
                .ok_or(PoleError::MathOverflow)?;
            token::mint_to(
                ctx.accounts.pole_lp_accounts.create_mint_to_context(
                    ctx.accounts.pole_authority.clone(),
                    ctx.accounts.token_program.clone(),
                    &[&[&[pole_pool.generic_config.bump as u8]]],
                ),
                mint_amount,
            )?;
            //reset deposit state
            pole_pool.port_state.reset();
            let last_sold_slot = pole_pool.port_state.last_sold_slot;
            if last_sold_slot == 0 {
                pole_pool.port_state.last_sold_slot = ctx.accounts.clock.slot
            };

            emit!(DidDeposit {
                liquidity_amount_deposited: user_liquidity_gain,
                lp_amount_minted: mint_amount,
            });
        }
        Ok(())
    }

    //assume reserve and obligation are refreshed
    #[access_control(valid_pole_pool(&ctx))]
    pub fn redeem_liquidity(ctx: Context<RedeemLiquidity>, amount: u64) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        if pole_pool.port_state.is_redeemed != 1 {
            if pole_pool.port_state.leverage == 0 {
                let port_exchange_rate = exchange_rate(&ctx.accounts.port_accounts.reserve)?;
                let port_liquidity = get_port_liquidity(
                    &port_exchange_rate,
                    &ctx.accounts.port_accounts.obligation,
                )?;

                let available_liquidity = token::accessor::amount(
                    &ctx.accounts.pole_liquidity_accounts.pole_liquidity_wallet,
                )?;

                pole_pool.port_state.init_port_liquidity = port_liquidity.0 .0;

                let exchange_rate = pole_pool
                    .basic_state
                    .exchange_rate(port_liquidity.try_add(available_liquidity.into())?)?;

                let redeem_liquidity_amount = exchange_rate.lp_to_liquidity(amount)?;

                pole_pool.port_state.redeem_amount = redeem_liquidity_amount;
                pole_pool.port_state.amount_to_unroll = pole_pool
                    .port_state
                    .redeem_amount
                    .saturating_sub(available_liquidity);
                //msg!("amount to unroll {:?}", pole_pool.port_state.amount_to_unroll);
                burn(
                    ctx.accounts.pole_lp_accounts.create_burn_context(
                        ctx.accounts.user_transfer_authority.to_account_info(),
                        ctx.accounts.token_program.clone(),
                        &[&[&[pole_pool.generic_config.bump as u8]]],
                    ),
                    amount,
                )?;
                pole_pool.basic_state.lp_amount = pole_pool
                    .basic_state
                    .lp_amount
                    .checked_sub(amount)
                    .ok_or(PoleError::MathOverflow)?;
            }

            if pole_pool.port_state.leverage > 0
                || (pole_pool.port_state.leverage == 0
                    && pole_pool.port_state.amount_to_unroll != 0)
            {
                let ltv = port_accessor::reserve_ltv(&ctx.accounts.port_accounts.reserve)?;
                let lending_leveraging_params = PortLendingLeveragingParams {
                    user_liquidity: &ctx.accounts.pole_liquidity_accounts.pole_liquidity_wallet,
                    obligation_owner: &ctx.accounts.pole_authority,
                    transfer_authority: &ctx.accounts.pole_authority,
                    clock: &ctx.accounts.clock.to_account_info(),
                    token_program: &ctx.accounts.token_program,
                };

                let repay_ratio = Decimal::from_percent(
                    ltv.checked_sub(pole_pool.port_config.port_reserve_percentage as u8)
                        .ok_or(PoleError::MathOverflow)?,
                );
                port_lending_unroll(
                    &ctx.accounts.port_accounts,
                    &lending_leveraging_params,
                    repay_ratio,
                    pole_pool.port_config.port_iterate as u8,
                    pole_pool.port_state.leverage as u8,
                    pole_pool.generic_config.bump as u8,
                    pole_pool.port_state.amount_to_unroll,
                )?;
            }

            if pole_pool.port_state.leverage
                == pole_pool
                    .port_config
                    .port_iterate
                    .checked_sub(1)
                    .ok_or(PoleError::MathOverflow)?
                || (pole_pool.port_state.leverage == 0
                    && pole_pool.port_state.amount_to_unroll == 0)
            {
                let fee_rate = Decimal::from_bips(pole_pool.generic_config.withdraw_fee_bips);

                let fee = fee_rate
                    .try_mul(pole_pool.port_state.redeem_amount)?
                    .try_ceil_u64()?;
                let redeem_exclude_fee = pole_pool
                    .port_state
                    .redeem_amount
                    .checked_sub(fee)
                    .ok_or(PoleError::MathOverflow)?;

                transfer(
                    {
                        let cpi_accounts = Transfer {
                            from: ctx
                                .accounts
                                .pole_liquidity_accounts
                                .pole_liquidity_wallet
                                .clone(),
                            to: ctx.accounts.pole_fee_account.clone(),
                            authority: ctx.accounts.pole_authority.clone(),
                        };
                        CpiContext::new_with_signer(
                            ctx.accounts.token_program.clone(),
                            cpi_accounts,
                            &[&[&[pole_pool.generic_config.bump as u8]]],
                        )
                    },
                    fee,
                )?;
                transfer(
                    ctx.accounts
                        .pole_liquidity_accounts
                        .create_transfer_pole_to_user_context(
                            ctx.accounts.pole_authority.clone(),
                            ctx.accounts.token_program.clone(),
                            &[&[&[pole_pool.generic_config.bump as u8]]],
                        ),
                    redeem_exclude_fee,
                )?;
                pole_pool.port_state.is_redeemed = 1;
                emit!(DidRedeem {
                    liquidity_amount_redeemed: pole_pool.port_state.redeem_amount,
                    lp_amount_burned: amount,
                });
            }
        };
        pole_pool.port_state.leverage = pole_pool
            .port_state
            .leverage
            .checked_add(1)
            .ok_or(PoleError::MathOverflow)?;
        if pole_pool.port_state.leverage == pole_pool.port_config.port_iterate {
            pole_pool.port_state.reset();
        }
        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn claim_and_sell(ctx: Context<ClaimAndSell>) -> ProgramResult {
        claim_reward(ctx.accounts.port_accounts.create_claim_reward_context(
            ctx.accounts.pole_authority.clone(),
            ctx.accounts.port_supply.clone(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.token_program.clone(),
            &[&[&[ctx.accounts.pole_pool.load()?.generic_config.bump as u8]]],
        ))
        .unwrap_or_else(|e| msg!("Unable to claim from port {:?}", e));

        let sell_amount = amount(&ctx.accounts.port_supply)?;
        let coin_lots = {
            let market = MarketState::load(
                &ctx.accounts.market_accounts.market,
                ctx.accounts.dex_program.key,
            )?;
            sell_amount
                .checked_div(market.coin_lot_size)
                .ok_or(PoleError::MathOverflow)?
        };
        if coin_lots != 0u64 {
            serum_swap::cpi::swap(
                ctx.accounts.create_swap_context(&[&[&[ctx
                    .accounts
                    .pole_pool
                    .load()?
                    .generic_config
                    .bump as u8]]]),
                Side::Ask,
                sell_amount,
                ExchangeRate {
                    rate: 0,
                    from_decimals: decimal(&ctx.accounts.port_mint)?,
                    quote_decimals: 0,
                    strict: false,
                },
            )
            .unwrap_or_else(|e| msg!("Swap error {:?}", e));
        }
        ctx.accounts.pole_pool.load_mut()?.port_state.last_sold_slot = ctx.accounts.clock.slot;

        emit!(DidSell {
            base_amount: sell_amount,
            slot: ctx.accounts.clock.slot
        });
        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn change_owner(ctx: Context<ChangeOwner>, owner: Pubkey) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        pole_pool.generic_config.owner = owner;
        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn change_fee_receiver(
        ctx: Context<ChangeFeeReceiver>,
        new_fee_receiver: Pubkey,
    ) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        pole_pool.generic_config.fee_receiver = new_fee_receiver;
        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn change_liquidity_cap(ctx: Context<ChangeLiquidityCap>, cap: u64) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        pole_pool.generic_config.liquidity_cap = cap;
        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn change_withdraw_fee(ctx: Context<ChangeWithdrawFee>, bips: u8) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        pole_pool.generic_config.withdraw_fee_bips = bips as u64;
        Ok(())
    }

    #[access_control(valid_pole_pool(&ctx))]
    pub fn change_min_deposit(ctx: Context<ChangeMinDeposit>, min_deposit: u64) -> ProgramResult {
        let pole_pool = &mut ctx.accounts.pole_pool.load_mut()?;
        pole_pool.port_config.min_deposit = min_deposit;
        Ok(())
    }

    pub fn create_user_balance(ctx: Context<CreateUserBalance>) -> ProgramResult {
        let user_balance = &mut ctx.accounts.user_balance;
        user_balance.user = ctx.accounts.user.key();
        Ok(())
    }

    pub fn add_balance(ctx: Context<AddBalance>, amount: u64) -> ProgramResult {
        let user_balance = &mut ctx.accounts.user_balance;
        user_balance.balance = user_balance
            .balance
            .checked_add(amount)
            .ok_or(PoleError::MathOverflow)?;
        user_balance.last_update = ctx.accounts.clock.slot;
        Ok(())
    }

    pub fn withdraw_balance(ctx: Context<WithdrawBalance>, amount: u64) -> ProgramResult {
        let user_balance = &mut ctx.accounts.user_balance;
        user_balance.balance = user_balance
            .balance
            .checked_sub(amount)
            .ok_or(PoleError::MathOverflow)?;
        user_balance.last_update = ctx.accounts.clock.slot;
        Ok(())
    }
}
