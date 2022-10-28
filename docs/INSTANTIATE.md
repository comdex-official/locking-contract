# Instantiate Operation

```rust
InstantiateMsg {
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
}
```

Instatiates a new instance of this contract with the following details. Here,
t1 and t2 represent two locking periods that may be used to lock sent tokens
in return for *vtokens*.

* `t1`--`t2` - specifies the duration (in seconds) and weight (in decimals) of each time period.
* `voting_period` - Proposal voting period.
* `vesting_contract` - Address of the vesting contract on chain.
* `foundation_addr` - An array of addresses of foundation wallets.
* `foundation_percentage` - Percentage of emission transferred to foundation.
* `surplus_asset_id` -
* `emission` -
* `admin` - Address of the admin.
* `min_lock_amount` - Minimum amount of tokens that need to be locked.
