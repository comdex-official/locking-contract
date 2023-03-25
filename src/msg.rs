use crate::state::{
    DelegationInfo, Emission, LockingPeriod, PeriodWeight, TokenInfo, Vote, Vtoken,
};
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct InstantiateMsg {
    pub t1: PeriodWeight,
    pub t2: PeriodWeight,
    pub voting_period: u64,
    pub vesting_contract: Addr,
    pub foundation_addr: Vec<String>,
    pub foundation_percentage: Decimal,
    pub surplus_asset_id: u64,
    pub emission: Emission,
    pub admin: Addr,
    pub min_lock_amount: Uint128,
    pub cswap_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    VoteProposal {
        app_id: u64,
        proposal_id: u64,
        extended_pair: Vec<u64>,
        ratio: Vec<Decimal>,
    },
    RaiseProposal {
        app_id: u64,
    },
    ClaimReward {
        app_id: u64,
        proposal_id: Option<u64>,
    },
    Bribe {
        proposal_id: u64,
        extended_pair: u64,
    },
    Emission {
        proposal_id: u64,
    },
    Rebase {
        proposal_id: u64,
    },
    Lock {
        app_id: u64,
        locking_period: LockingPeriod,
        recipient: Option<Addr>,
    },
    Withdraw {
        denom: String,
    },
    Transfer {
        recipient: String,
        locking_period: LockingPeriod,
        denom: String,
    },
    Delegate {
        delegation_address: Addr,
        denom: String,
        ratio: Decimal,
    },
    Undelegate {
        delegation_address: Addr,
        denom: String,
    },
    UpdateProtocolFees {
        delegate_address: Addr,
        fees: Decimal,
    },
    UserDelegation {
        address: Addr,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
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
        height: Option<u64>,
    },
    State {},
    Emission {
        app_id: u64,
    },
    ExtendedPairVote {
        proposal_id: u64,
        extended_pair_id: u64,
    },
    UserProposalAllUp {
        proposal_id: u64,
        address: Addr,
    },
    Rebase {
        address: Addr,
        app_id: u64,
        denom: String,
    },
    Admin {},
    EmissionRewards {
        proposal_id: u64,
    },
    ProjectedEmission {
        proposal_id: u64,
        app_id: u64,
        gov_token_denom: String,
        gov_token_id: u64,
    },
    DelegationRequest {
        delegated_address: Addr,
        delegator_address: Addr,
        height: Option<u64>,
    },
    DelegatorParamRequest {
        delegated_address: Addr,
    },
    GetEmissionVotingPower {
        address: Addr,
        proposal_id: u64,
        denom: String,
    },
    DelegationStats {
        delegated_address: Addr,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SudoMsg {
    UpdateVestingContract {
        address: Addr,
    },
    UpdateEmissionRate {
        emission_rate: Decimal,
        app_id: u64,
    },
    UpdateFoundationInfo {
        addresses: Vec<String>,
        foundation_percentage: Decimal,
    },
    UpdateLockingPeriod {
        t1: PeriodWeight,
        t2: PeriodWeight,
    },
    UpdateAdmin {
        admin: Addr,
    },
    UpdateVotingPeriod {
        voting_period: u64,
    },
    AddNewDelegation {
        delegation_info: DelegationInfo,
    },
    UpdateExistingDelegation {
        delegation_info: DelegationInfo,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct MigrateMsg {
    pub cswap_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct IssuedNftResponse {
    pub nft: TokenInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct RebaseResponse {
    pub proposal_id: u64,
    pub rebase_amount: Uint128,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ProposalVoteRespons {
    pub proposal_pair_data: Vec<Vote>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ProposalPairVote {
    pub extended_pair_id: u64,
    pub my_vote: Uint128,
    pub total_vote: Uint128,
    pub bribe: Vec<Coin>,
}
