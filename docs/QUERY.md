# Query Operations

The following query operations are available:

1. IssuedNft
2. IssuedVtokens
3. Supply
4. CurrentProposal
5. Proposal
6. BribeByProposal
7. HasVoted
8. Vote
9. ClaimableBribe
10. Withdrawable
11. TotalVTokens
12. State
13. Emisson
14. ExtendedPairVote

## IssuedNft

```rust
IssuedNft {
    address: String,
}
```

Queries the nft info for the given address.

* `address` - Address of the user.

RESPONSE:

```rust
TokenInfo {
    pub owner: Addr,
    pub token_id: u64,
}
```

* `owner` - Address of this NFT.
* `token_id` - Unique identifier assigned.

## IssuedVtokens

```rust
IssuedVtokens {
    address: Addr,
    denom: String,
    start_after: u32,
    limit: Option<u32>,
}
```

Query the vtokens issued to the user for a specific denomination.

* `address` - Address of the user.
* `denom` - Denomination of token. For example, OSMO, CMDX, etc.
* `start_after` - Returns results after this index.
* `limit` - Count of results in response.

RESPONSE:

The response contains an array of vtoken with the following details.

```rust
Vtoken {
    pub token: Coin,
    pub vtoken: Coin,
    pub period: LockingPeriod,
    pub start_time: Timestamp,
    pub end_time: Timestamp,
    pub status: Status,
}
```

* `token` - Original token denomination and quantity locked.
* `vtoken` - Corresponding token denomination and quantity released.
* `period` - Locking period for the locked tokens, i.e. t1 to t4.
* `start_time` - Timestamp when the tokens were locked.
* `end_time` - Timestamp when the tokens will be unlocked.
* `status` - Current status of the tokens, i.e. *locked* or *unlocked*.

## Supply

```rust
Supply {
    denom: String,
}
```

Query the total supply of a locked denomination and the corresponding supply of
the released vtokens.

* `denom` - Denomination of locked token.

RESPONSE:

```rust
TokenSupply {
    pub token: u128,
    pub vtoken: u128,
}
```

* `token` - Total tokens locked for the specified denomination.
* `vtoken` - Total vtokens released corresponding to the locked tokens.

## CurrentProposal

```rust
CurrentProposal {
    app_id: u64,
}
```

Query the latest proposal specific to an application.

* `app_id` - Application ID.

RESPONSE:

The response of this query returns an integer denoting the unique proposal ID.

## Proposal

```rust
Proposal {
    proposal_id: u64,
}
```

Query the proposal with the specified proposal ID.

* `proposal_id` - Unique proposal ID.

```rust
Proposal {
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
```

* `app_id` - Application ID where this proposal was raised.
* `voting_start_time` - Timestamp when the voting starts for the proposal.
* `voting_end_time` - Timestamp when the voting ends for the proposal.
* `extended_pair` - unique identifier of the token pair for which to vote.
* `emission_completed` - Boolean value that represent if the emission has been
calculated for the proposal.
* `rebase_completed` - Boolean value to represent if rebase has been calculated.
* `foundation_emission_completed` - Binary value to represent if foundation
emission has been calculated.
* `emission_distributed` - Total emission distributed thus far.
* `rebase_distributed` - Total rebase distributed thus far.
* `foundation_distributed` - Total foundation emission distributed thus far.
* `total_voted_weight` - Total weight of the votes for this proposal.
* `total_surplus` - Total reward surplus.
* `height` - Block height when the proposal was raised.

## BribeByProposal

```rust
BribeByProposal {
    proposal_id: u64,
    extended_pair_id: u64,
}
```

Query the bribes made by users on this proposal. If bribes are present, then
the response is as shown under *RESPONSE*, else the query return *None*.

* `proposal_id` - Unique proposal ID.
* `extended_pair_id` - Unique extended pair ID.

RESPONSE:

The response of this query is an array of tokens specifying the amount and
denomination.

```rust
{
    denom: String,
    amount: Uint128,
}
```

* `denom` - Denomination of the token used for bribe.
* `amount` - Amount of tokens used for bribe.

## HasVoted

```rust
HasVoted {
    address: Addr,
    proposal_id: u64,
},
```

Query a proposal if the specified user has voted.

* `address` - Address of the user.
* `proposal_id` - Unique proposal ID.

RESPONSE:

If the user has voted then, it returns *true*, else *false*.

## Vote

```rust
Vote {
    address: Addr,
    proposal_id: u64,
},
```

Query information on a users vote. If the user did vote, then
the response is as shown under *RESPONSE*, else the query returns *None*.

* `address` - Address of the user.
* `proposal_id` - Unique proposal ID.

```rust
{
    app_id: u64,
    extended_pair: u64,
    vote_weight: u128,
}
```

* `app_id` - Unique application ID.
* `extended_pair` - Unique ID of the extended pair for which the user voted.
* `vote_weight` - Weight of the user's vote.

## ClaimableBribe

```rust
ClaimableBribe {
    address: Addr,
    app_id: u64,
},
```

Query the claimable bribe for a user for all completed proposals of an application.

* `address` - Address of the user.
* `app_id` - Unique application ID.

RESPONSE:

The response of this query is an array of tokens specifying the denomination
and the amount.

```rust
{
    denom: String,
    amount: Uint128,
}
```

* `denom` - Denomination of the token used for bribe.
* `amount` - Amount of tokens used for bribe.

## Withdrawable

```rust
Withdrawable {
    address: String,
    denom: String,
}
```

Query the total withdrawable tokens for a specific denom, which were previously
locked.

* `address` - Address of the user.
* `denom` - Denomination for which to check unlocked tokens.

RESPONSE:

```rust
{
    amount: Coin,
}
```

* `amount` - Contains the total amount withdrawable.

## TotalVTokens

```rust
TotalVTokens {
    address: Addr,
    denom: String,
    height: Option<u64>,
},
```

Query the total vtokens in possession of a user, for a specific denom and
optionally for a specific block height. This allows querying past data.

* `address` - Address of the user.
* `denom` - Denomination for which to check. For example, OSMO, CMDX, etc.
* `height` - Block height at which to check the vtoken balance.

RESPONSE:

This query returns the total amount of vtokens.

## State

```rust
State {}
```

Query the state configuration for the contract. This is helpful to retrieve
information regarding the locking periods (t1 - t4), the number of NFT issued
or the vesting contract address among others.

```rust
State {
    pub t1: PeriodWeight,
    pub t2: PeriodWeight,
    pub t3: PeriodWeight,
    pub t4: PeriodWeight,
    pub num_tokens: u64,
    pub vesting_contract: Addr,
    pub foundation_addr: Vec<String>,
    pub foundation_percentage: Decimal,
    pub voting_period: u64,
    pub surplus_asset_id: u64,
    pub min_lock_amount: Uint128,
}
```

* `t1`--`t4` - Locking periods.
* `num_tokens` - Current count of NFTs issued.
* `vesting_contract` - Address of the vesting contract.
* `foundation_addr` - Address(es) of the foundation wallets.
* `foundation_percentage` - Percentage of emission transferred to foundation.
* `voting_period` - Maximum voting period for any proposal.
* `surplus_asset_id` -
* `min_lock_amount` - Minimum amount of tokens that need to be locked.

## Emission

```rust
Emission {
    app_id: u64,
}
```

Query the emission status for the given application.

* `app_id` - Unique Application ID.

RESPONSE:

```rust
Emission {
    app_id: u64,
    total_rewards: u128,
    rewards_pending: u128,
    emission_rate: Decimal,
    distributed_rewards: u128,
}
```

* `app_id` - Unique application ID.
* `total_rewards` - Total rewards that need to be distributed.
* `rewards_pending` - Rewards yet to be distributed.
* `emission_rate` - Rate at which emission is calculated.
* `distributed_rewards` - Rewards distributed of the total rewards.

## ExtendedPairVote

```rust
ExtendedPairVote {
    proposal_id: u64,
    extended_pair_id: u64,
}
```

Queries the votes received for the specified extended pair and proposal.

* `proposal_id` - Unique proposal ID.
* `extended_pair_id` - Unique ID of the extended pair.

RESPONSE:

The response of this query returns the total votes as integer.

## UserProposalAllUp

```rust
UserProposalAllUp {
    proposal_id: u64,
    address: Addr,
}
```

Query the proposal data for user.

* `proposal_id` - Unique proposal ID.
* `address` - Address of the user.

```rust
ProposalPairVote {
    extended_pair_id: u64,
    my_vote: Uint128,
    total_vote: Uint128,
    bribe: Vec<Coin>,
}
```

Query the proposal vote for a pair for an user.

* `extended_pair` - Unique ID of the extended pair for which the user voted.
* `my_vote` - Vote weight of user.
* `total_vote` - Total weight of votes cast.
* `bribe` - List of extenal incentives on pair.

## Rebase

```rust
Rebase {
    address: Addr,
    app_id: u64,
    denom: String,
}
```

Query the total rebase for a user.

* `address` - Address of the user.
* `app_id` - Unique application ID.
* `denom` - Denomination of the token.

RESPONSE:

The response of this query is an array of the following details.

```rust
RebaseResponse {
    proposal_id: u64,
    rebase_amount: Uint128,
}
```

* `proposal_id` - Unique proposal ID.
* `rebase_amount` - Rebase amount that may be claimed.
