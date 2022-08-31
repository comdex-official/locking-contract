use crate::state::{Emission, LockingPeriod, PeriodWeight, TokenInfo, Vtoken};
use cosmwasm_std::{Addr, Coin, Decimal};
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
    pub foundation_addr: Vec<String>,
    pub foundation_percentage: Decimal,
    pub surplus_asset_id: u64,
    pub emission: Emission,
    pub admin: Addr,
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
    ClaimReward {
        app_id: u64,
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
        app_id: u64,
    },
    Lock {
        app_id: u64,
        locking_period: LockingPeriod,
    },
    Withdraw {
        denom: String,
    },
    Transfer {
        recipent: String,
        locking_period: LockingPeriod,
        denom: String,
    },
    FoundationRewards {
        proposal_id: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Query the NFT
    IssuedNft {
        address: String,
    },

    /// Query the total vtokens issued to a single user.
    IssuedVtokens {
        address: Addr,
        denom: String,
        start_after: u32,
        limit: Option<u32>,
    },
    VestedTokens {
        denom: String,
    },

    Supply {
        denom: String,
    },
    CurrentProposal {
        app_id: u64,
    },
    Proposal {
        proposal_id: u64,
    },
    BribeByProposal {
        proposal_id: u64,
        extended_pair_id: u64,
    },
    HasVoted {
        address: Addr,
        proposal_id: u64,
    },
    Vote {
        address: Addr,
        proposal_id: u64,
    },

    ClaimableBribe {
        address: Addr,
        app_id: u64,
    },

    /// Total amount of given denom withdrawable.
    Withdrawable {
        address: String,
        denom: String,
    },
    TotalVTokens {
        address: Addr,
        denom: String,
        height: Option<u64>
    },
    State {},
    Emission {
        app_id: u64,
    },
    ExtendedPairVote {
        proposal_id: u64,
        extended_pair_id: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SudoMsg {
    UpdateVestingContract {
        address: Addr,
    },
    UpdateEmissionRate {
        emission: Emission,
    },
    UpdateFoundationInfo {
        addresses: Vec<Addr>,
        foundation_percentage: Decimal,
    },
    UpdateLockingPeriod {
        t1: PeriodWeight,
        t2: PeriodWeight,
        t3: PeriodWeight,
        t4: PeriodWeight,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct IssuedNftResponse {
    pub nft: TokenInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WithdrawableResponse {
    pub amount: Coin,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnlockedTokensResponse {
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
