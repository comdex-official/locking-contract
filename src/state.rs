use cosmwasm_std::{Addr, Timestamp};
use cosmwasm_std::{Coin, Decimal, Uint128};
use cw_controllers::Admin;
use cw_storage_plus::{Item, Map, SnapshotMap, Strategy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const VOTEPOWER: SnapshotMap<(&Addr, String), Uint128> = SnapshotMap::new(
    "voters_key",
    "voters_checkpoints",
    "voters_changelogs",
    Strategy::EveryBlock,
);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub struct PeriodWeight {
    pub period: u64,
    pub weight: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LockingPeriod {
    T1,
    T2,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// When the tokens are in the locking period.
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
    /// Locking period i.e. T1 or T2
    pub period: LockingPeriod,
    /// Time at which the tokens were locked
    pub start_time: Timestamp,
    /// Point in time after which the tokens can be unlocked
    pub end_time: Timestamp,
    /// Current status of the tokens
    pub status: Status,
}

/// NFT struct for holding the token info
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub struct TokenInfo {
    /// Owner of the NFT
    pub owner: Addr,
    /// Unique token id
    pub token_id: u64,
}

/// Contains the four locking periods and the unlock period.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub struct State {
    pub t1: PeriodWeight,
    pub t2: PeriodWeight,
    pub num_tokens: u64,
    pub vesting_contract: Addr,
    pub foundation_addr: Vec<String>,
    pub foundation_percentage: Decimal,
    pub voting_period: u64,
    pub surplus_asset_id: u64,
    pub min_lock_amount: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
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
pub const ADMIN: Admin = Admin::new("admin_address");

pub const TOKENS: Map<Addr, TokenInfo> = Map::new("tokens");
// Total supply of each (vtoken supplied, token deposited)
pub const SUPPLY: SnapshotMap<&str, TokenSupply> = SnapshotMap::new(
    "supply",
    "supply_checkpoints",
    "supply_changelogs",
    Strategy::EveryBlock,
);
// Vtoken owned by an address for a specific denom
pub const VTOKENS: SnapshotMap<(Addr, &str), Vec<Vtoken>> = SnapshotMap::new(
    "owner_vtoken",
    "voters_checkpoints",
    "voters_changelogs",
    Strategy::EveryBlock,
);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Proposal {
    pub app_id: u64,
    pub voting_start_time: Timestamp,
    pub voting_end_time: Timestamp,
    pub extended_pair: Vec<u64>,
    pub emission_completed: bool,
    pub rebase_completed: bool,
    pub foundation_emission_completed: bool,
    pub emission_distributed: u128,
    pub rebase_distributed: u128,
    pub foundation_distributed: u128,
    pub total_voted_weight: u128,
    pub total_surplus: Coin,
    pub height: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct Emission {
    pub app_id: u64,
    pub total_rewards: u128,
    pub rewards_pending: u128,
    pub emission_rate: Decimal,
    pub distributed_rewards: u128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct Reward {
    pub id: u64,
    pub reward: Uint128,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct EmissionVaultPool {
    pub app_id: u64,
    pub pool_ids: Vec<u64>,
    pub vault_ids: Vec<u64>,
    pub total_emission_rewards: Uint128,
    pub pool_rewards: Vec<Uint128>,
    pub vault_rewards: Vec<Uint128>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct Vote {
    pub app_id: u64,
    pub extended_pair: u64,
    pub vote_weight: u128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct Delegation {
    pub delegated_to: Addr,
    pub delegated_at: Timestamp,
    pub delegation_end_at: Timestamp,
    pub delegated: u128

}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Eq)]
pub struct DelegationInfo {
    pub delegation_name: String,
    pub total_delegated: u128
  
}


pub const PROPOSALCOUNT: Item<u64> = Item::new("proposal_count");

pub const CSWAP_ID: Item<u64> = Item::new("cswap_id");

pub const APPCURRENTPROPOSAL: Map<u64, u64> = Map::new("app_current_proposal");

pub const PROPOSALVOTE: Map<(u64, u64), Uint128> = Map::new("proposal_vote");

pub const PROPOSAL: Map<u64, Proposal> = Map::new("proposal");

pub const BRIBES_BY_PROPOSAL: Map<(u64, u64), Vec<Coin>> = Map::new("bribes_by_proposal");

pub const EMISSION: Map<u64, Emission> = Map::new("emission");

pub const EMISSION_REWARD: Map<u64, EmissionVaultPool> = Map::new("emission_rewards");

pub const VOTERS_VOTE: Map<(Addr, u64), bool> = Map::new("voters_vote");

pub const VOTERSPROPOSAL: Map<(Addr, u64), Vote> = Map::new("voters_proposal");

pub const MAXPROPOSALCLAIMED: Map<(u64, Addr), u64> = Map::new("max_proposal_claimed");

pub const COMPLETEDPROPOSALS: Map<u64, Vec<u64>> = Map::new("completed_proposals");

pub const REBASE_CLAIMED: Map<(Addr, u64), bool> = Map::new("rebase_claimed");

pub const DELEGATED: SnapshotMap<Addr, Delegation> = SnapshotMap::new(
    "delegated",
    "voters_checkpoints",
    "voters_changelogs",
    Strategy::EveryBlock,
);

pub const DELEGATION_INFO: SnapshotMap<Addr, DelegationInfo> = SnapshotMap::new(
    "delegation_info",
    "voters_checkpoints",
    "voters_changelogs",
    Strategy::EveryBlock,
);


