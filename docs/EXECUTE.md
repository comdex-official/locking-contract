# Execute Operations

The following execute operations are available:

1. Lock
2. Withdraw
3. Transfer
4. VoteProposal
5. RaiseProposal
6. ClaimReward
7. Bribe
8. Emission
9. Rebase
10. FoundationRewards

## Lock

```rust
Lock {
    app_id: u64,
    locking_period: LockingPeriod,
    recipient: Option<Addr>,
}
```

Allows locking funds sent along with the execute message. The funds will be
locked for the specified locking period and optionally for the specified user.

* `app_id` - Unique application ID.
* `locking_period` - Choice of locking period for locking funds.
* `recipient` - Optionally set the owner of the locked funds. If not specified,
then the tokens will be locked for the user that initiated the transaction.

## Withdraw

```rust
Withdraw {
    denom: String,
}
```

Any tokens for the specified denomination may be withdrawn, if the tokens
have unlocked, i.e. competed their locking period.

* `denom` - Token denomination that is to be withdrawn.

## Transfer

```rust
Transfer {
    recipient: String,
    locking_period: LockingPeriod,
    denom: String,
}
```

Any locked/unlocked (unlocked tokens that haven't been withdrawn) tokens may be
transferred to another user. Transferring locked tokens will only be withdrawable
when the locking period for the tokens has been completed.

* `recipient` - Address of the recipient.
* `locking_period` - Tokens with the specified locking period will be transferred.
* `denom` - Token denomination that needs to be transferred.

## VoteProposal

```rust
VoteProposal {
    app_id: u64,
    proposal_id: u64,
    extended_pair: u64,
}
```

Any user with vtokens may vote on an active proposal.

* `app_id` - Unique application ID.
* `proposal_id` - Unique proposal ID of an active proposal.
* `extended_pair` - Unique ID of the extended pair to vote.

## Bribe

```rust
Bribe {
    proposal_id: u64,
    extended_pair: u64,
}
```

Incentivise users to vote for a specific extended pair, by bribing the said pair.

* `proposal_id` - Unique proposal ID of an active proposal.
* `extended_pair` - Unique ID of the extended pair to bribe.

## ClaimReward

```rust
ClaimReward {
    app_id: u64,
}
```

vtoken holders are eligible for surplus funds received from the protocol. Whereas,
voters receive proportional bribe for the extended pair they voted upon.
ClaimReward facilitates a user to claim rewards for previous proposals as well, if they have not been claimed.

* `app_id` - Unique application ID.

## Rebase

```rust
Rebase {
    proposal_id: u64,
},
```

vtoken holders are incentivised with more vtoken to avoid their voting power dilution. Each vtoken holder is rebased to their proportional individual vtoken holding.

* `proposal_id` - Unique proposal ID for which to rebase.

## RaiseProposal

```rust
RaiseProposal {
    app_id: u64,
}
```

An admin is allowed to raise a new proposal for the specific application. Furthermore, only a single proposal may be active at any given moment. Any new
proposal will not be raised until the previously active proposal has been
completed.

* `app_id` - Unique application ID.

## Emission

```rust
Emission {
    proposal_id: u64,
}
```

It is a process in which a certain amount of the token (governance token for the app) will be minted after every proposal. The Emission distribution is computed as:

* **Emission distribution:** They are distributed to vault owners based on the share of votes received for their respective vault pair id.

  rewards_pending*(emission_rate)*(1-total_vtoken/circulating_supply)*(1-foundation_percentage)

* **Foundation distribution:** They are distributed to foundation_addr equally.

  Rebase distribution: rewards_pending*(emission_rate)*(total_vtoken/circulating_supply)

**NOTE:** Only the admin is allowed to execute this transaction.

* `proposal_id` - Unique proposal ID for which to calculate the emission.

## FoundationRewards

```rust
FoundationRewards {
    proposal_id: u64,
},
```

This triggers the foundation reward disbursal once the Emission calculation has completed.

**NOTE:** Only the admin is allowed to execute this transaction.

* `proposal_id` - Unique proposal ID for which to distribute the emission.
