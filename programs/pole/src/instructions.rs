use std::cell::Ref;
use std::mem::size_of;

use crate::adaptors::supply;
use crate::error::PoleError;
use crate::states::{PolePortPool, UserBalance, DISCRIMINATOR_SIZE};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program_pack::Pack;
use anchor_spl::dex;
use anchor_spl::dex::serum_dex::state::OpenOrders;
use anchor_spl::dex::InitOpenOrders;
use anchor_spl::token::{Burn, MintTo, Transfer};
use port_anchor_adaptor::port_accessor::{
    is_obligation_stale, is_reserve_stale, obligation_borrows_count, obligation_deposits_count,
};
use port_anchor_adaptor::*;
use port_lending::state::Obligation;
use port_staking::state::StakeAccount;
use port_staking_instructions as port_staking;
use port_variable_rate_lending_instructions as port_lending;
use serum_swap::cpi::accounts::{MarketAccounts as SwapMarketAccounts, Swap};

const STALE_SLOT: u64 = 50;
const MIN_LIQUIDITY_CAP: u64 = 1_000_000;
#[derive(AnchorDeserialize, AnchorSerialize, Debug)]
pub struct InitParams {
    pub liquidity_cap: u64,
    pub withdraw_fee_bips: u8,
    pub port_iterate: u8,
    pub port_reserve_percentage: u8,
    pub port_min_deposit: u64,
    pub swap_program: Pubkey,
    pub reserve: Pubkey,
}

#[derive(AnchorDeserialize, AnchorSerialize, Debug, PartialEq, Copy, Clone)]
pub struct StakingBumps {
    pub pool_bump: u8,
    pub authority_bump: u8,
    pub port_pool_bump: u8,
    pub lp_bump: u8,
    pub reward_token_pool_bump: u8,
}

const SERUM_PADDING: usize = 12;

pub trait PolePortAccounts {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError>;
}

#[derive(Accounts, Clone)]
#[instruction(authority_bump: u8, name: String, pda_bump: u8, init_params: InitParams)]
pub struct CreatePool<'info> {
    #[account(init, seeds = [name.as_ref()], bump = pda_bump, payer = user, space = size_of::< PolePortPool > () + DISCRIMINATOR_SIZE,
        constraint = init_params.port_iterate < 10 && init_params.port_iterate >= 1 @ PoleError::InvalidPoolConfig,
        constraint = init_params.liquidity_cap >= MIN_LIQUIDITY_CAP @ PoleError::InvalidPoolConfig,
        constraint = init_params.port_reserve_percentage <= 20 && init_params.port_reserve_percentage >= 1 @ PoleError::InvalidPoolConfig,
        constraint = init_params.port_min_deposit > 50 @ PoleError::InvalidPoolConfig,
    )
    ]
    pub pole_pool: AccountLoader<'info, PolePortPool>,

    #[account(init, payer=user, mint::authority=pole_authority, mint::decimals=6)]
    pub lp_mint: AccountInfo<'info>,

    #[account(init, payer=user, token::authority=pole_authority, token::mint=liquidity_mint)]
    pub liquidity_supply: AccountInfo<'info>,

    #[account(init, payer=user, token::authority=pole_authority, token::mint=port_lp_mint)]
    pub port_lp_supply: AccountInfo<'info>,

    #[account(init, payer=user, token::authority=pole_authority, token::mint=port_mint)]
    pub port_supply: AccountInfo<'info>,

    #[account(init, payer=user, token::authority=user, token::mint=liquidity_mint)]
    pub fee_receiver: AccountInfo<'info>,

    #[account(init, payer=user, owner=dex::ID, space=size_of::<OpenOrders>() + SERUM_PADDING,)]
    pub port_open_orders: AccountInfo<'info>,

    #[account(owner=token_program.key())]
    pub liquidity_mint: AccountInfo<'info>,

    #[account(owner=token_program.key())]
    pub port_lp_mint: AccountInfo<'info>,

    #[account(owner=token_program.key())]
    pub port_mint: AccountInfo<'info>,

    #[account(seeds=[], bump=authority_bump)]
    pub pole_authority: AccountInfo<'info>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(owner=dex_program.key())]
    pub dex_market: AccountInfo<'info>,

    #[account(executable)]
    pub token_program: AccountInfo<'info>,

    #[account(executable)]
    pub dex_program: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
    pub rent: AccountInfo<'info>,
}
#[derive(Accounts, Clone)]
#[instruction()]
pub struct InitPortAccounts<'info> {
    #[account(mut,
    constraint = pole_pool.load()?.generic_config.owner == user.key() @ PoleError::InvalidOwner)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,

    #[account(init, payer=user, owner=port_lending_program.key(), space=Obligation::LEN)]
    pub obligation: AccountInfo<'info>,

    #[account(init, payer=user, owner=port_staking_program.key(), space=StakeAccount::LEN)]
    pub stake_account: AccountInfo<'info>,

    #[account(seeds = [], bump = pole_pool.load() ?.generic_config.bump as u8)]
    pub pole_authority: AccountInfo<'info>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(executable)]
    pub token_program: AccountInfo<'info>,

    #[account(executable)]
    pub port_lending_program: AccountInfo<'info>,

    #[account(executable)]
    pub port_staking_program: AccountInfo<'info>,

    #[account(owner=port_lending_program.key())]
    pub port_lending_market: AccountInfo<'info>,

    #[account(owner=port_staking_program.key())]
    pub staking_pool: AccountInfo<'info>,

    pub rent: AccountInfo<'info>,
    pub clock: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts, Clone)]
#[instruction()]
pub struct VerifyDeposit<'info> {
    #[account(mut)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub transaction_info: AccountInfo<'info>,
}

#[derive(Accounts, Clone)]
#[instruction()]
pub struct VerifyRedeem<'info> {
    #[account(mut)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    #[account(address = anchor_lang::solana_program::sysvar::instructions::id())]
    pub transaction_info: AccountInfo<'info>,
}

//assume reserve and obligation are refreshed
#[derive(Accounts, Clone)]
#[instruction(amount: u64)]
pub struct DepositLiquidity<'info> {
    #[account(mut,
        constraint = pole_pool.load()?.port_state.last_sold_slot == 0 || STALE_SLOT >= clock.slot.checked_sub(pole_pool.load()?.port_state.last_sold_slot).ok_or(PoleError::MathOverflow)?  @ PoleError::PortNotSell,
        constraint = pole_pool.load()?.generic_config.liquidity_supply
            == pole_liquidity_accounts.pole_liquidity_wallet.key() @ PoleError::InvalidLiquidityWallet,
        constraint = pole_pool.load()?.generic_config.lp_mint
            == pole_lp_accounts.lp_mint.key() @ PoleError::InvalidLPMint,
        constraint = pole_pool.load()?.port_config.obligation
            == port_accounts.obligation.key() @ PoleError::InvalidObligation,
        constraint = pole_pool.load()?.port_config.stake_account
            == port_accounts.stake_account.key() @ PoleError::InvalidStakeAccount,
        constraint = pole_pool.load()?.port_config.port_lending_program
            == port_accounts.port_lending_program.key() @ PoleError::InvalidPortLendingProgram,
        constraint = pole_pool.load()?.generic_config.token_program
            == token_program.key() @ PoleError::InvalidTokenProgram,
        constraint = pole_pool.load()?.port_state.deposit_verified == 1 @ PoleError::DepositNotVerified,
        constraint = (pole_pool.load()?.port_state.leverage == 0) ^ (amount == 0) @ PoleError::DepositAmountInvalid,
        constraint = pole_pool.load()?.basic_state.lp_amount == supply(&pole_lp_accounts.lp_mint)? @ PoleError::LPMintAmountNotMatch,
        constraint = pole_pool.load()?.port_state.approved_wallet == pole_liquidity_accounts.user_liquidity_wallet.key() @ PoleError::WrongWallet,
        constraint = !is_obligation_stale(&port_accounts.obligation)? ^ (amount == 0) @ PoleError::ObligationStale,
        constraint = !is_reserve_stale(&port_accounts.reserve)? @ PoleError::ReserveStale
    )]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    #[account(seeds = [], bump = pole_pool.load() ?.generic_config.bump as u8)]
    pub pole_authority: AccountInfo<'info>,
    pub pole_liquidity_accounts: PoleLiquidityAccounts<'info>,
    pub pole_lp_accounts: PoleLPAccounts<'info>,
    #[account(constraint = port_accounts.reserve.key() == pole_pool.load()?.port_config.reserve)]
    pub port_accounts: PortLendingAccounts<'info>,
    #[account(mut)]
    pub user_transfer_authority: Signer<'info>,
    #[account(executable)]
    pub token_program: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
}

//assume reserve and obligation are refreshed
#[derive(Accounts, Clone)]
#[instruction(amount: u64)]
pub struct RedeemLiquidity<'info> {
    #[account(mut,
        constraint = STALE_SLOT >= clock.slot.checked_sub(pole_pool.load()?.port_state.last_sold_slot).ok_or(PoleError::MathOverflow)? @ PoleError::PortNotSell,
        constraint = (amount == 0) ^ (pole_pool.load()?.port_state.leverage == 0) @ PoleError::RedeemAmountInvalid,
        constraint = pole_pool.load()?.generic_config.liquidity_supply == pole_liquidity_accounts.pole_liquidity_wallet.key() @ PoleError::InvalidLiquidityWallet,
        constraint = pole_pool.load()?.port_config.obligation == port_accounts.obligation.key() @ PoleError::InvalidObligation,
        constraint = pole_pool.load()?.port_config.stake_account == port_accounts.stake_account.key() @ PoleError::InvalidStakeAccount,
        constraint = pole_pool.load()?.port_config.port_lending_program == port_accounts.port_lending_program.key() @ PoleError::InvalidPortLendingProgram,
        constraint = pole_pool.load()?.generic_config.token_program == token_program.key() @ PoleError::InvalidTokenProgram,
        constraint = pole_pool.load()?.generic_config.fee_receiver == pole_fee_account.key() @ PoleError::InvalidFeeAccount,
        constraint = pole_pool.load()?.port_state.redeem_verified == 1 @ PoleError::RedeemNotVerified,
        constraint = pole_pool.load()?.basic_state.lp_amount == supply(&pole_lp_accounts.lp_mint)? @ PoleError::LPMintAmountNotMatch,
        constraint = pole_pool.load()?.port_state.approved_wallet == pole_lp_accounts.user_lp_wallet.key() @ PoleError::WrongWallet
    )]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    #[account(seeds = [], bump = pole_pool.load() ?.generic_config.bump as u8)]
    pub pole_authority: AccountInfo<'info>,
    #[account(mut)]
    pub pole_fee_account: AccountInfo<'info>,
    pub pole_liquidity_accounts: PoleLiquidityAccounts<'info>,
    pub pole_lp_accounts: PoleLPAccounts<'info>,
    #[account(constraint = port_accounts.reserve.key() == pole_pool.load()?.port_config.reserve)]
    pub port_accounts: PortLendingAccounts<'info>,
    #[account(mut)]
    pub user_transfer_authority: Signer<'info>,
    #[account(executable)]
    pub token_program: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts, Clone)]
#[instruction()]
pub struct ClaimAndSell<'info> {
    #[account(mut,
        constraint = pole_pool.load()?.generic_config.liquidity_supply
            == liquidity_supply.key() @ PoleError::InvalidLiquidityWallet,
        constraint = pole_pool.load()?.port_config.port_supply
            == port_supply.key() @ PoleError::InvalidPortWallet,
        constraint = pole_pool.load()?.port_config.stake_account
            == port_accounts.stake_account.key() @ PoleError::InvalidStakeAccount,
        constraint = pole_pool.load()?.port_config.port_staking_program
            == port_accounts.port_staking_program.key() @ PoleError::InvalidStakingProgram,
        constraint = pole_pool.load()?.generic_config.token_program
            == token_program.key() @ PoleError::InvalidTokenProgram,
        constraint = pole_pool.load()?.serum_config.dex_program
            == dex_program.key() @ PoleError::InvalidDexProgram,
        constraint = pole_pool.load()?.serum_config.swap_program
            == swap_program.key() @ PoleError::InvalidSwapProgram,
    )]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    #[account(seeds = [], bump = pole_pool.load() ?.generic_config.bump as u8)]
    pub pole_authority: AccountInfo<'info>,
    #[account(mut, owner=token_program.key())]
    pub liquidity_supply: AccountInfo<'info>,
    #[account(mut, owner=token_program.key())]
    pub port_supply: AccountInfo<'info>,
    pub market_accounts: MarketAccounts<'info>,
    pub port_accounts: PortStakingAccounts<'info>,
    pub port_mint: AccountInfo<'info>,
    #[account(executable)]
    pub token_program: AccountInfo<'info>,
    #[account(executable)]
    pub dex_program: AccountInfo<'info>,
    #[account(executable)]
    pub swap_program: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub rent: AccountInfo<'info>,
}
#[derive(Accounts, Clone)]
#[instruction(new_owner: Pubkey)]
pub struct ChangeOwner<'info> {
    #[account(mut, constraint = pole_pool.load()?.generic_config.owner == owner.key() @ PoleError::InvalidOwner)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    pub owner: Signer<'info>,
}

#[derive(Accounts, Clone)]
#[instruction(new_fee_receiver: Pubkey)]
pub struct ChangeFeeReceiver<'info> {
    #[account(mut, constraint = pole_pool.load()?.generic_config.owner == owner.key() @ PoleError::InvalidOwner)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    pub owner: Signer<'info>,
}

#[derive(Accounts, Clone)]
#[instruction(cap: u64)]
pub struct ChangeLiquidityCap<'info> {
    #[account(mut, constraint = pole_pool.load()?.generic_config.owner == owner.key() @ PoleError::InvalidOwner, constraint = cap >= MIN_LIQUIDITY_CAP @ PoleError::InvalidPoolConfig,)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    pub owner: Signer<'info>,
}

#[derive(Accounts, Clone)]
#[instruction(bips: u8)]
pub struct ChangeWithdrawFee<'info> {
    #[account(mut, constraint = pole_pool.load()?.generic_config.owner == owner.key() @ PoleError::InvalidOwner)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    pub owner: Signer<'info>,
}

#[derive(Accounts, Clone)]
#[instruction(min_deposit: u64)]
pub struct ChangeMinDeposit<'info> {
    #[account(mut, constraint = pole_pool.load()?.generic_config.owner == owner.key() @ PoleError::InvalidOwner)]
    pub pole_pool: AccountLoader<'info, PolePortPool>,
    pub owner: Signer<'info>,
}
#[derive(Accounts, Clone)]
pub struct MarketAccounts<'info> {
    #[account(mut)]
    pub market: AccountInfo<'info>,
    #[account(mut)]
    pub open_orders: AccountInfo<'info>,
    #[account(mut)]
    pub request_queue: AccountInfo<'info>,
    #[account(mut)]
    pub event_queue: AccountInfo<'info>,
    #[account(mut)]
    pub bids: AccountInfo<'info>,
    #[account(mut)]
    pub asks: AccountInfo<'info>,
    #[account(mut)]
    pub coin_vault: AccountInfo<'info>,
    #[account(mut)]
    pub pc_vault: AccountInfo<'info>,
    pub vault_signer: AccountInfo<'info>,
}

#[derive(Accounts, Clone)]
pub struct PortStakingAccounts<'info> {
    pub staking_program_authority: AccountInfo<'info>,
    #[account(executable)]
    pub port_staking_program: AccountInfo<'info>,
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    #[account(mut)]
    pub staking_pool: AccountInfo<'info>,
    #[account(mut)]
    pub reward_supply: AccountInfo<'info>,
}

#[derive(Accounts, Clone)]
pub struct PoleLiquidityAccounts<'info> {
    #[account(mut)]
    pub user_liquidity_wallet: AccountInfo<'info>,
    #[account(mut)]
    pub pole_liquidity_wallet: AccountInfo<'info>,
}

#[derive(Accounts, Clone)]
pub struct PoleLPAccounts<'info> {
    #[account(mut)]
    pub lp_mint: AccountInfo<'info>,
    #[account(mut)]
    pub user_lp_wallet: AccountInfo<'info>,
}

#[derive(Accounts, Clone)]
pub struct PortLendingAccounts<'info> {
    #[account(mut)]
    pub user_lp_wallet: AccountInfo<'info>,
    #[account(mut)]
    pub lp_mint: AccountInfo<'info>,
    #[account(mut)]
    pub reserve_lp_wallet: AccountInfo<'info>,
    pub liquidity_mint: AccountInfo<'info>,
    #[account(mut)]
    pub reserve_liquidity_wallet: AccountInfo<'info>,
    #[account(mut,
        constraint = obligation_deposits_count(&obligation).map(|c| c <= 1).unwrap_or(false),
        constraint = obligation_borrows_count(&obligation).map(|c| c <= 1).unwrap_or(false),
    )]
    pub obligation: AccountInfo<'info>,
    #[account(mut)]
    pub reserve: AccountInfo<'info>,
    #[account(mut)]
    pub reserve_fee: AccountInfo<'info>,
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    #[account(mut)]
    pub staking_pool: AccountInfo<'info>,
    pub lending_market: AccountInfo<'info>,
    pub lending_market_authority: AccountInfo<'info>,
    #[account(executable)]
    pub port_lending_program: AccountInfo<'info>,
    #[account(executable)]
    pub port_staking_program: AccountInfo<'info>,
}

#[derive(Accounts, Clone)]
#[instruction(bump: u8, pool_pubkey: Pubkey)]
pub struct CreateUserBalance<'info> {
    #[account(init,payer = user, seeds=[user.key().as_ref(),pool_pubkey.as_ref()], bump=bump, space = size_of::<UserBalance>() + DISCRIMINATOR_SIZE)]
    pub user_balance: Account<'info, UserBalance>,
    #[account(mut)]
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts, Clone)]
#[instruction(amount: u64)]
pub struct AddBalance<'info> {
    #[account(mut, has_one=user)]
    pub user_balance: Account<'info, UserBalance>,
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts, Clone)]
#[instruction(amount: u64)]
pub struct WithdrawBalance<'info> {
    #[account(mut, has_one=user, constraint = user_balance.balance >= amount @ PoleError::NotEnoughBalance)]
    pub user_balance: Account<'info, UserBalance>,
    pub user: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub clock: Sysvar<'info, Clock>,
}

impl<'info> InitPortAccounts<'info> {
    pub fn create_init_obligation_cpi<'a, 'b, 'c>(
        &self,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, InitObligation<'info>> {
        let init = InitObligation {
            obligation: self.obligation.clone(),
            lending_market: self.port_lending_market.clone(),
            obligation_owner: self.pole_authority.clone(),
            rent: self.rent.clone(),
            spl_token_id: self.token_program.clone(),
            clock: self.clock.clone(),
        };
        CpiContext::new_with_signer(self.port_lending_program.clone(), init, seeds)
    }

    pub fn create_create_stake_account_cpi<'a, 'b, 'c>(
        &self,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, CreateStakeAccount<'info>> {
        let init = CreateStakeAccount {
            staking_pool: self.staking_pool.clone(),
            stake_account: self.stake_account.clone(),
            owner: self.pole_authority.clone(),
            rent: self.rent.clone(),
        };
        CpiContext::new_with_signer(self.port_staking_program.clone(), init, seeds)
    }
}

impl<'a> PolePortAccounts for InitPortAccounts<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}
impl<'info> CreatePool<'info> {
    pub fn create_init_open_orders_cpi<'a, 'b, 'c>(
        &self,
        open_orders: AccountInfo<'info>,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, InitOpenOrders<'info>> {
        let init = InitOpenOrders {
            open_orders,
            authority: self.pole_authority.clone(),
            market: self.dex_market.clone(),
            rent: self.rent.clone(),
        };
        CpiContext::new_with_signer(self.dex_program.clone(), init, seeds)
    }
}

impl<'a> PolePortAccounts for DepositLiquidity<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}

impl<'a> PolePortAccounts for ChangeFeeReceiver<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}

impl<'a> PolePortAccounts for RedeemLiquidity<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}

impl<'a> PolePortAccounts for ChangeOwner<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}
impl<'a> PolePortAccounts for ChangeLiquidityCap<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}

impl<'a> PolePortAccounts for ChangeMinDeposit<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}
impl<'a> PolePortAccounts for ChangeWithdrawFee<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}

impl<'info> PoleLPAccounts<'info> {
    pub(crate) fn create_mint_to_context<'a, 'b, 'c>(
        &self,
        authority: AccountInfo<'info>,
        token_program: AccountInfo<'info>,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.lp_mint.clone(),
            to: self.user_lp_wallet.clone(),
            authority,
        };
        CpiContext::new_with_signer(token_program, cpi_accounts, seeds)
    }
    pub(crate) fn create_burn_context<'a, 'b, 'c>(
        &self,
        authority: AccountInfo<'info>,
        token_program: AccountInfo<'info>,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.lp_mint.clone(),
            to: self.user_lp_wallet.clone(),
            authority,
        };
        CpiContext::new_with_signer(token_program, cpi_accounts, seeds)
    }
}

impl<'info> PoleLiquidityAccounts<'info> {
    pub(crate) fn create_transfer_user_to_pole_context<'a, 'b, 'c>(
        &self,
        user_authority: AccountInfo<'info>,
        token_program: AccountInfo<'info>,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.user_liquidity_wallet.clone(),
            to: self.pole_liquidity_wallet.clone(),
            authority: user_authority,
        };
        CpiContext::new_with_signer(token_program, cpi_accounts, seeds)
    }
    pub(crate) fn create_transfer_pole_to_user_context<'a, 'b, 'c>(
        &self,
        pole_authority: AccountInfo<'info>,
        token_program: AccountInfo<'info>,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.pole_liquidity_wallet.clone(),
            to: self.user_liquidity_wallet.clone(),
            authority: pole_authority,
        };
        CpiContext::new_with_signer(token_program, cpi_accounts, seeds)
    }
}

impl<'info> PortStakingAccounts<'info> {
    pub(crate) fn create_claim_reward_context<'a, 'b, 'c>(
        &self,
        stake_account_owner: AccountInfo<'info>,
        reward_dest: AccountInfo<'info>,
        clock: AccountInfo<'info>,
        token_program: AccountInfo<'info>,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, ClaimReward<'info>> {
        let cpi_accounts = ClaimReward {
            stake_account_owner,
            stake_account: self.stake_account.clone(),
            staking_pool: self.staking_pool.clone(),
            reward_token_pool: self.reward_supply.clone(),
            reward_dest,
            staking_program_authority: self.staking_program_authority.clone(),
            clock,
            token_program,
        };
        CpiContext::new_with_signer(self.port_staking_program.clone(), cpi_accounts, seeds)
    }
}

#[derive(Clone)]
pub struct PortLendingLeveragingParams<'info, 'a>
where
    'info: 'a,
{
    pub user_liquidity: &'a AccountInfo<'info>,
    pub obligation_owner: &'a AccountInfo<'info>,
    pub transfer_authority: &'a AccountInfo<'info>,
    pub clock: &'a AccountInfo<'info>,
    pub token_program: &'a AccountInfo<'info>,
}

impl<'info> PortLendingAccounts<'info> {
    pub(crate) fn create_deposit_and_collateralize_context<'a, 'b, 'c, 'd>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        seeds: &'b [&'c [&'d [u8]]],
    ) -> CpiContext<'b, 'c, 'd, 'info, DepositAndCollateralize<'info>> {
        let cpi_accounts = DepositAndCollateralize {
            source_liquidity: params.user_liquidity.clone(),
            user_collateral: self.user_lp_wallet.clone(),
            reserve: self.reserve.clone(),
            reserve_liquidity_supply: self.reserve_liquidity_wallet.clone(),
            reserve_collateral_mint: self.lp_mint.clone(),
            lending_market: self.lending_market.clone(),
            lending_market_authority: self.lending_market_authority.clone(),
            destination_collateral: self.reserve_lp_wallet.clone(),
            obligation: self.obligation.to_account_info(),
            obligation_owner: params.obligation_owner.clone(),
            stake_account: self.stake_account.clone(),
            staking_pool: self.staking_pool.clone(),
            transfer_authority: params.transfer_authority.clone(),
            clock: params.clock.clone(),
            token_program: params.token_program.clone(),
            port_staking_program: self.port_staking_program.clone(),
        };
        CpiContext::new_with_signer(self.port_lending_program.clone(), cpi_accounts, seeds)
    }
    pub(crate) fn create_borrow_context<'a, 'b, 'c, 'd>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        seeds: &'b [&'c [&'d [u8]]],
    ) -> CpiContext<'b, 'c, 'd, 'info, Borrow<'info>> {
        let cpi_accounts = Borrow {
            source_liquidity: self.reserve_liquidity_wallet.clone(),
            destination_liquidity: params.user_liquidity.clone(),
            reserve: self.reserve.clone(),
            reserve_fee_receiver: self.reserve_fee.clone(),
            lending_market: self.lending_market.clone(),
            lending_market_authority: self.lending_market_authority.clone(),
            obligation: self.obligation.to_account_info(),
            obligation_owner: params.obligation_owner.clone(),
            clock: params.clock.clone(),
            token_program: params.token_program.clone(),
        };
        CpiContext::new_with_signer(self.port_lending_program.clone(), cpi_accounts, seeds)
    }
    pub(crate) fn create_repay_context<'a, 'b, 'c, 'd>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        seeds: &'b [&'c [&'d [u8]]],
    ) -> CpiContext<'b, 'c, 'd, 'info, Repay<'info>> {
        let cpi_accounts = Repay {
            source_liquidity: params.user_liquidity.clone(),
            destination_liquidity: self.reserve_liquidity_wallet.clone(),
            reserve: self.reserve.clone(),
            obligation: self.obligation.to_account_info(),
            lending_market: self.lending_market.clone(),
            transfer_authority: params.transfer_authority.clone(),
            clock: params.clock.clone(),
            token_program: params.token_program.clone(),
        };
        CpiContext::new_with_signer(self.port_lending_program.clone(), cpi_accounts, seeds)
    }
    pub(crate) fn create_withdraw_context<'a, 'b, 'c, 'd>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        seeds: &'b [&'c [&'d [u8]]],
    ) -> CpiContext<'b, 'c, 'd, 'info, Withdraw<'info>> {
        let cpi_accounts = Withdraw {
            source_collateral: self.reserve_lp_wallet.clone(),
            destination_collateral: self.user_lp_wallet.clone(),
            reserve: self.reserve.clone(),
            lending_market: self.lending_market.clone(),
            lending_market_authority: self.lending_market_authority.clone(),
            stake_account: self.stake_account.clone(),
            obligation: self.obligation.to_account_info(),
            obligation_owner: params.obligation_owner.clone(),
            clock: params.clock.clone(),
            token_program: params.token_program.clone(),
            staking_pool: self.staking_pool.clone(),
            port_staking_program: self.port_staking_program.clone(),
        };
        CpiContext::new_with_signer(self.port_lending_program.clone(), cpi_accounts, seeds)
    }
    pub(crate) fn create_redeem_context<'a, 'b, 'c, 'd>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        seeds: &'b [&'c [&'d [u8]]],
    ) -> CpiContext<'b, 'c, 'd, 'info, Redeem<'info>> {
        let cpi_accounts = Redeem {
            source_collateral: self.user_lp_wallet.clone(),
            destination_liquidity: params.user_liquidity.clone(),
            reserve: self.reserve.clone(),

            reserve_collateral_mint: self.lp_mint.clone(),
            reserve_liquidity_supply: self.reserve_liquidity_wallet.clone(),
            lending_market: self.lending_market.clone(),
            lending_market_authority: self.lending_market_authority.clone(),

            clock: params.clock.clone(),
            token_program: params.token_program.clone(),
            transfer_authority: params.transfer_authority.clone(),
        };
        CpiContext::new_with_signer(self.port_lending_program.clone(), cpi_accounts, seeds)
    }
    pub(crate) fn create_fresh_reserve_context<'a>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        oracles: Vec<AccountInfo<'info>>,
    ) -> CpiContext<'_, '_, '_, 'info, RefreshReserve<'info>> {
        let cpi_accounts = RefreshReserve {
            reserve: self.reserve.clone(),
            clock: params.clock.clone(),
        };

        let mut context = CpiContext::new(self.port_lending_program.clone(), cpi_accounts);
        context.remaining_accounts.extend(oracles);
        context
    }
    pub(crate) fn create_refresh_obligation_context<'a>(
        &self,
        params: &PortLendingLeveragingParams<'info, 'a>,
        reserves: Vec<&AccountInfo<'info>>,
    ) -> CpiContext<'_, '_, '_, 'info, RefreshObligation<'info>> {
        let cpi_accounts = RefreshObligation {
            obligation: self.obligation.clone(),
            clock: params.clock.clone(),
        };
        let mut context = CpiContext::new(self.port_lending_program.clone(), cpi_accounts);
        context
            .remaining_accounts
            .extend(reserves.into_iter().cloned());
        context
    }
}
impl<'a> PolePortAccounts for ClaimAndSell<'a> {
    fn get_pole_pool(&self) -> Result<Ref<PolePortPool>, ProgramError> {
        self.pole_pool.load()
    }
}

impl<'info, 'a, 'b, 'c> ClaimAndSell<'info> {
    pub(crate) fn create_swap_context(
        &self,
        seeds: &'a [&'b [&'c [u8]]],
    ) -> CpiContext<'a, 'b, 'c, 'info, Swap<'info>> {
        let cpi_accounts = Swap {
            market: SwapMarketAccounts {
                market: self.market_accounts.market.clone(),
                open_orders: self.market_accounts.open_orders.clone(),
                request_queue: self.market_accounts.request_queue.clone(),
                event_queue: self.market_accounts.event_queue.clone(),
                bids: self.market_accounts.bids.clone(),
                asks: self.market_accounts.asks.clone(),
                order_payer_token_account: self.port_supply.clone(),
                coin_vault: self.market_accounts.coin_vault.clone(),
                pc_vault: self.market_accounts.pc_vault.clone(),
                vault_signer: self.market_accounts.vault_signer.clone(),
                coin_wallet: self.port_supply.clone(),
            },
            authority: self.pole_authority.clone(),
            pc_wallet: self.liquidity_supply.clone(),
            dex_program: self.dex_program.clone(),
            token_program: self.token_program.clone(),
            rent: self.rent.clone(),
        };
        CpiContext::new_with_signer(self.swap_program.clone(), cpi_accounts, seeds)
    }
}
