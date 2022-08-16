use cosmwasm_std::{Addr, Timestamp};
use cosmwasm_std::{Coin, Decimal, Uint128};
use cw_storage_plus::{Item, Map};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PeriodWeight {
    pub period: u64,
    pub weight: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LockingPeriod {
    T1,
    T2,
    T3,
    T4,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// When the tokens are in the vesting period.
    Locked,
    /// When the tokens have completed the locking period,
    /// the owner is free to retrieve their tokens.
    Unlocked,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Vtoken {
    /// amount of token being locked
    pub token: Coin,
    /// amount of vtoken created
    pub vtoken: Coin,
    /// Locking period i.e. T1..4
    pub period: LockingPeriod,
    /// Time at which the tokens were locked
    pub start_time: Timestamp,
    /// Point in time after which the tokens can be unlocked
    pub end_time: Timestamp,
    /// Current status of the tokens
    pub status: Status,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallType {
    /// Update only the amount in the Vtoken if another deposit for the same
    /// locking period exists.
    UpdateAmount,
    /// Update amount and time period in the Vtoken, if another deposit for the
    /// same locking period exists.
    UpdatePeriod,
}

/// NFT struct for holding the token info
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenInfo {
    /// Owner of the NFT
    pub owner: Addr,
    /// All Vtokens for a user
    pub vtokens: Vec<Vtoken>,
    /// Unique token id
    pub token_id: u64,
}

/// Contains the four locking periods and the unlock period.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct State {
    pub t1: PeriodWeight,
    pub t2: PeriodWeight,
    pub t3: PeriodWeight,
    pub t4: PeriodWeight,
    pub num_tokens: u64,
    pub vesting_contract: Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TokenSupply {
    // total token in the system.
    pub token: u128,
    // total vtoken released, for the corresponding token, in the system
    pub vtoken: u128,
}

// The following mappings are as follows
// Holds the internal state
pub const STATE: Item<State> = Item::new("state");
// Owner to NFT
pub const TOKENS: Map<Addr, TokenInfo> = Map::new("tokens");
// Total supply of each (vtoken supplied, token deposited)
pub const SUPPLY: Map<&str, TokenSupply> = Map::new("supply");
// Vtoken owned by an address for a specific denom
pub const VTOKENS: Map<(Addr, &str), Vec<Vtoken>> = Map::new("Vtokens by owner");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Proposal {
    pub app_id: u64,
    pub voting_start_time: u64,
    pub voting_end_time: u64,
    pub extended_pair: Vec<u64>,
    pub emission_completed: bool,
    pub rebase_completed: bool,
    pub emission_distributed: u128,
    pub rebase_distributed: u128,
    pub total_voted_weight: u128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Emission {
    pub app_id: u64,
    pub total_rewards: u128,
    pub rewards_pending: u128,
    pub emmission_rate: Decimal,
    pub distributed_rewards: u128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Vote {
    pub app_id: u64,
    pub extended_pair: u64,
    pub vote_weight: u128,
    pub bribe_claimed: bool,
}

pub const VOTINGPERIOD: Item<u64> = Item::new("Voting_period");

pub const PROPOSALCOUNT: Item<u64> = Item::new("Voting_period");

pub const APPCURRENTPROPOSAL: Map<u64, u64> = Map::new("App_Current_proposal");

pub const PROPOSALVOTE: Map<(u64, u64), Uint128> = Map::new("Proposal vote");

pub const PROPOSAL: Map<u64, Proposal> = Map::new("Proposal vote");

pub const BRIBES_BY_PROPOSAL: Map<(u64, u64), Vec<Coin>> = Map::new("Proposal vote");

pub const EMISSION: Map<u64, Emission> = Map::new("Proposal vote");

pub const VOTERS_VOTE: Map<(Addr, u64), bool> = Map::new("has voted");

pub const VOTERSPROPOSAL: Map<(Addr, u64), Vote> = Map::new("has voted");
