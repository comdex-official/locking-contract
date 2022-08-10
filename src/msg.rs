use crate::state::{CallType, LockingPeriod, PeriodWeight, TokenInfo, Vtoken};
use cosmwasm_std::{
    to_binary, Addr, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response, StdResult,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub t1: PeriodWeight,
    pub t2: PeriodWeight,
    pub t3: PeriodWeight,
    pub t4: PeriodWeight,
    pub voting_period: u64,
    pub vesting_contract: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    VoteProposal {
        app_id: u64,
        proposal_id: u64,
        extended_pair: u64,
    },
    RaiseProposal {
        app_id: u64,
    },
    ClaimBribe {
        proposal_id: u64,
    },
    Bribe {
        proposal_id: u64,
        extended_pair: u64,
    },
    Emmission {
        proposal_id: u64,
    },
    Rebase {
        proposal_id: u64,
    },
    ClaimRebase {
        proposal_id: u64,
    },
    Lock {
        app_id: u64,
        locking_period: LockingPeriod,
        calltype: Option<CallType>,
    },
    Withdraw {
        app_id: u64,
        denom: String,
        amount: u64,
        lockingperiod: LockingPeriod,
    },
    TransferOwnership{
        recipent:Addr,
        locking_period:LockingPeriod,
        denom:String
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Query the NFT
    IssuedNft {
        address: String,
    },

    /// Query the tokens with Unlocked status. If denom is supplied, then only
    /// query for a specific denomination, else return all tokens.
    UnlockedTokens {
        address: Option<String>,
        denom: Option<String>,
    },

    /// Query the tokens with Locked status. If denom is supplied, the only
    /// query for a specific denomination, else return all tokens.
    LockedTokens {
        address: Option<String>,
        denom: Option<String>,
    },

    /// Query the total vtokens issued to a single user.
    IssuedVtokens {
        address: Option<String>,
    },
    VestedTokens {
        denom: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct IssuedNftResponse {
    pub nft: TokenInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnlockedTokensResponse {
    pub tokens: Vec<Coin>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnlockingTokensResponse {
    pub tokens: Vec<Coin>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct LockedTokensResponse {
    pub tokens: Vec<Coin>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct IssuedVtokensResponse {
    pub vtokens: Vec<Vtoken>,
}
