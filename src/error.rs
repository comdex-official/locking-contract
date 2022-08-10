use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Custom Error val: {val:?}")]
    CustomError { val: String },
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
    #[error("{msg:?}")]
    NotFound { msg: String },

    #[error("Insufficient funds: {funds}")]
    InsufficientFunds { funds: u128 },

    #[error("The locking period is still active.")]
    TimeNotOvered {},

    #[error("The token is Allready Unlocked.")]
    AllreadyUnLocked {},

    #[error("The token is in Locked State.")]
    NotUnlocked {},

    #[error("The token is not in Locked state")]
    NotLocked {},
}
