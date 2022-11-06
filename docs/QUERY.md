# Overview

This contract provides the functionality of locking funds for predefined durations
and returning *vtokens*.

Each new account is assigned a Non-Fungible Token (NFT).

Term Definitions:

* token - It refers to the Native or CW20 token. For example, OSMO, CMDX, etc.
* vtoken -
* emission
* rebase
* extended pair

## Locking

During locking, there are 3 important factors that need to be considered:

1. **Denomination** - Only governance token may be locked.
2. **Locking period** - The amount of *vtoken* generated is calculated using the chosen locking period's.
3. **Owner** - It is possible to lock tokens for another user by specifying the
`recipient` in the execute message. By specifying the aforementioned parameter,
the owner of the tokens will be set to that address. Therefore, the original
sender will not be able to withdraw or use any other functionality enjoyed by the
`recipient`.

### Caluculation of vtokens

There are two locking periods available, henceforth referred to as T1 and T2.
Both locking periods have two values associated with them, *period* and *weight*.
*Period* refers to the minimum locking duration of the tokens, in seconds. Withdrawals are not
allowed while the tokens are locked. However, it is upto the user when to withdraw
their tokens post unlocking.
*Weight* is used to calculate the vtokens based on the original deposit of tokens.

For example, given the following assumptions:

> **T1** = { "period": 100, "weight": 0.5 }  
> **T2** = { "period": 200, "weight": 1.0 }

If the user sends 100 HARBOR for **T1** locking period then, the generated
vHARBOR is calculated as:

> vHARBOR = deposit * weight of locking period
>
> vHARBOR = 100 * 0.5 = 50

## Voting

This contract allows for voting on a token pair to recieve external incentives.
Only vtoken holders are allowed to vote during an epoch. The weight of a users
vote is based on the amount of vtoken available at the beginning of the epoch.

## Bribing

To incentivise vtoken holders to vote on one token-pair over another, users are
allowed to bribe that token-pair. The total bribe is distributed among all the
voters based on the proportional value of their vote weight over the total vote
weight.

## Withdrawing

Once the deposited tokens have completed their locking period, they may be
withdrawn by simply providing the denomination of the token. All
deposited tokens, irrespective of their locking periods, are transferred to the
user.