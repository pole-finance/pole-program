use anchor_lang::prelude::*;

#[error]
pub enum PoleError {
    // 300
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Pool state is invalid")]
    InvalidPoolState,
    #[msg("Pool config is invalid")]
    InvalidPoolConfig,
    #[msg("Obligation collaterals are not those expected")]
    InvalidPortObligationCollaterals,
    #[msg("Obligation liquidities are not those expected")]
    InvalidPortObligationLiquidity,
    // 305
    #[msg("Deposit more!")]
    PortDepositAmountTooSmall,
    #[msg("Invalid transaction supplied")]
    InvalidTransaction,
    #[msg("Invalid redeem instruction")]
    InvalidRedeem,
    #[msg("Call ClaimAndSell first")]
    PortNotSell,
    #[msg("InvalidLiquidityWallet")]
    InvalidLiquidityWallet,
    // 310
    #[msg("InvalidLPMint")]
    InvalidLPMint,
    #[msg("InvalidObligation")]
    InvalidObligation,
    #[msg("InvalidStakeAccount")]
    InvalidStakeAccount,
    #[msg("InvalidPortLendingProgram")]
    InvalidPortLendingProgram,
    #[msg("InvalidTokenProgram")]
    InvalidTokenProgram,
    // 315
    #[msg("InvalidStakingProgram")]
    InvalidStakingProgram,
    #[msg("InvalidDexProgram")]
    InvalidDexProgram,
    #[msg("InvalidSwapProgram")]
    InvalidSwapProgram,
    #[msg("DepositNotVerified")]
    DepositNotVerified,
    #[msg("RedeemNotVerified")]
    RedeemNotVerified,
    // 320
    #[msg("DepositAmountInvalid")]
    DepositAmountInvalid,
    #[msg("RedeemAmountInvalid")]
    RedeemAmountInvalid,
    #[msg("LPMintAmountNotMatch")]
    LPMintAmountNotMatch,
    #[msg("WrongWallet")]
    WrongWallet,
    #[msg("InvalidFeeAccount")]
    InvalidFeeAccount,
    // 325
    #[msg("InvalidPortWallet")]
    InvalidPortWallet,
    #[msg("InvalidOwner")]
    InvalidOwner,
    #[msg("NotEnoughBalance")]
    NotEnoughBalance,
    #[msg("InvalidStakeOwner")]
    InvalidStakeOwner,
    #[msg("ReserveStale, plz refresh")]
    ReserveStale,

    //330
    #[msg("ObligationStale, plz refresh")]
    ObligationStale,

    #[msg("MeetDepositLimit")]
    MeetDepositLimit,
}
