#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, QueryRequest,
    Response, Storage, Timestamp, Uint128, WasmQuery,
};
use cw2::set_contract_version;
use schemars::_serde_json::de;
use std::ops::{AddAssign, Div, Mul, Sub, SubAssign};

use crate::error::ContractError;
use crate::helpers::{get_token_supply, query_app_exists, query_get_asset_data};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{
    CallType, Proposal, Vote, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, EMISSION, PROPOSAL,
    PROPOSALCOUNT, PROPOSALVOTE, VOTERSPROPOSAL, VOTERS_VOTE, VOTINGPERIOD,
};
use crate::state::{
    LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, LOCKED, STATE,
    SUPPLY, TOKENS, UNLOCKED, VTOKENS,
};
use comdex_bindings::{ComdexMessages, ComdexQuery};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:{{project-name}}";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        t1: msg.t1,
        t2: msg.t2,
        t3: msg.t3,
        t4: msg.t4,
        num_tokens: 0,
        vesting_contract: msg.vesting_contract,
    };

    STATE.save(deps.storage, &state)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    VOTINGPERIOD.save(deps.storage, &msg.voting_period)?;
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        // ExecuteMsg::VoteProposal {
        //     app_id,
        //     proposal_id,
        //     extended_pair,
        // } => vote_proposal(deps, env, info, app_id, proposal_id, extended_pair),
        ExecuteMsg::RaiseProposal { app_id } => raise_proposal(deps, env, info, app_id),

        ExecuteMsg::Bribe {
            proposal_id,
            extended_pair,
        } => bribe_proposal(deps, env, info, proposal_id, extended_pair),

        ExecuteMsg::ClaimBribe { proposal_id } => {
            claim_bribe_proposal(deps, env, info, proposal_id)
        }

        ExecuteMsg::Emmission { proposal_id } => emission(deps, env, info, proposal_id),

        ExecuteMsg::Rebase { proposal_id } => emission(deps, env, info, proposal_id),

        ExecuteMsg::ClaimRebase { proposal_id } => emission(deps, env, info, proposal_id),

        ExecuteMsg::Lock {
            app_id,
            locking_period,
            calltype,
        } => handle_lock_nft(deps, env, info, app_id, locking_period, calltype),

        ExecuteMsg::Withdraw {
            app_id,
            denom,
            amount,
            lockingperiod,
        } => handle_withdraw(deps, env, info, denom, amount, lockingperiod),

        ExecuteMsg::TransferOwnership {
            recipent,
            locking_period,
            denom,
            calltype,
        } => handle_transfer_ownership(deps, env, info, recipent, locking_period, denom, calltype),
        _ => panic!("Not implemented"),
    }
}

fn lock_funds(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    app_id: u64,
    sender: Addr,
    funds: Coin,
    locking_period: LockingPeriod,
    calltype: Option<CallType>,
) -> Result<(), ContractError> {
    let mut state = STATE.load(deps.storage)?;

    // Load the locking period and weight
    let PeriodWeight { period, weight } = get_period(state.clone(), locking_period.clone())?;

    // Loads the NFT if present else None
    let nft = TOKENS.may_load(deps.storage, sender.clone())?;

    match nft {
        Some(mut token) => {
            let res: Vec<&Vtoken> = token
                .vtokens
                .iter()
                .filter(|s| s.token.denom == funds.denom.clone() && s.period == locking_period)
                .collect();

            if res.is_empty() {
                // create new token
                let new_vtoken = create_vtoken(
                    deps.storage,
                    &env,
                    locking_period.clone(),
                    period,
                    weight,
                    funds.clone(),
                )?;

                // Save the vtoken in VTOKENS
                VTOKENS.save(
                    deps.storage,
                    (sender.clone(), &funds.denom),
                    &vec![new_vtoken.clone()],
                )?;

                // Update nft and save
                token.vtokens.push(new_vtoken);
                TOKENS.save(deps.storage, sender.clone(), &token)?;
            } else {
                let mut vtoken = res[0].to_owned();

                let mut remaining: Vec<Vtoken> = token
                    .vtokens
                    .into_iter()
                    .filter(|s| !(s.token.denom == funds.denom))
                    .collect();

                // Increase the token count
                vtoken.token.amount.add_assign(funds.amount.clone());

                // Increase the vtoken count
                let updated_vtoken = weight * funds.amount;
                vtoken.vtoken.amount.add_assign(updated_vtoken);

                // start_time and end_time are updated along with tokens
                if let CallType::UpdatePeriod = calltype.unwrap_or(CallType::UpdateAmount) {
                    vtoken.start_time = env.block.time;
                    vtoken.end_time = env.block.time.plus_seconds(period);
                }

                remaining.push(vtoken);
                token.vtokens = remaining;

                update_denom_supply(
                    deps.storage,
                    &funds.denom,
                    updated_vtoken.u128(),
                    funds.amount.u128(),
                    true,
                )?;

                TOKENS.save(deps.storage, sender.clone(), &token)?;
            }

            // Finally update the coins in locked mapping
            update_locked(
                deps.storage,
                sender.clone(),
                funds.denom.clone(),
                funds.amount,
                true,
            )?;
        }
        None => {
            // Create a new NFT
            state.num_tokens += 1;

            let mut new_nft = TokenInfo {
                owner: sender.clone(),
                vtokens: vec![],
                token_id: state.num_tokens,
            };

            // Create new Vtoken for new deposit
            let new_vtoken = create_vtoken(
                deps.storage,
                &env,
                locking_period.clone(),
                period,
                weight,
                funds.clone(),
            )?;

            VTOKENS.save(
                deps.storage,
                (sender.clone(), &funds.denom),
                &vec![new_vtoken.clone()],
            )?;

            LOCKED.save(deps.storage, sender.clone(), &vec![funds.clone()])?;

            new_nft.vtokens.push(new_vtoken);
            TOKENS.save(deps.storage, sender.clone(), &new_nft)?;
        }
    }

    Ok(())
}

/// Lock the sent tokens and create corresponding vtokens
pub fn handle_lock_nft(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    locking_period: LockingPeriod,
    calltype: Option<CallType>,
) -> Result<Response, ContractError> {
    // Only allow a single denomination
    if info.funds.is_empty() {
        return Err(ContractError::InsufficientFunds { funds: 0 });
    } else if info.funds.len() > 1 {
        return Err(ContractError::CustomError {
            val: String::from("Multiple denominations are not supported as yet."),
        });
    }

    if info.funds[0].amount.is_zero() {
        return Err(ContractError::InsufficientFunds { funds: 0 });
    }

    lock_funds(
        deps,
        env,
        app_id,
        info.sender.clone(),
        info.funds[0].clone(),
        locking_period,
        calltype,
    )?;

    Ok(Response::new()
        .add_attribute("action", "lock")
        .add_attribute("from", info.sender))
}

fn create_vtoken(
    storage: &mut dyn Storage,
    env: &Env,
    locking_period: LockingPeriod,
    period: u64,
    weight: Decimal,
    funds: Coin,
) -> Result<Vtoken, ContractError> {
    // Create the vtoken
    let mut vdenom = String::from("v");
    vdenom.push_str(&funds.denom);

    let amount = weight * funds.amount;

    update_denom_supply(
        storage,
        &funds.denom,
        amount.u128(),
        funds.amount.u128(),
        true,
    )?;

    Ok(Vtoken {
        token: funds.clone(),
        vtoken: Coin {
            denom: vdenom,
            amount,
        },
        period: locking_period,
        start_time: env.block.time,
        end_time: env.block.time.plus_seconds(period),
        status: Status::Locked,
    })
}

/// Update the SUPPLY map for the toal supply for vtokens and the corresponding
/// tokens locked.
fn update_denom_supply(
    storage: &mut dyn Storage,
    denom: &str,
    vquantity: u128,
    quantity: u128,
    add: bool,
) -> Result<(), ContractError> {
    // Load the total supply in the for the given denom
    let denom_supply = SUPPLY.may_load(storage, denom)?;
    if let None = denom_supply {
        if !add {
            return Err(ContractError::NotFound {
                msg: "vTokens don't exist for the given denom".to_string(),
            });
        }
    }
    // Create new struct if not present in SUPPLY
    let mut denom_supply_struct = denom_supply.unwrap_or(TokenSupply {
        token: 0,
        vtoken: 0,
    });

    if add {
        denom_supply_struct.vtoken += vquantity;
        denom_supply_struct.token += quantity;
    } else {
        if denom_supply_struct.vtoken < vquantity {
            return Err(ContractError::InsufficientFunds { funds: vquantity });
        } else if denom_supply_struct.token < quantity {
            return Err(ContractError::InsufficientFunds { funds: quantity });
        }

        denom_supply_struct.vtoken -= vquantity;
        denom_supply_struct.token -= quantity;
    }

    SUPPLY.save(storage, denom, &denom_supply_struct)?;

    Ok(())
}

/// Update the LOCKED mapping.
fn update_locked(
    storage: &mut dyn Storage,
    owner: Addr,
    denom: String,
    amount: Uint128,
    add: bool,
) -> Result<(), ContractError> {
    let coin_vector = LOCKED.may_load(storage, owner.clone())?;
    let mut coin_vector = coin_vector.unwrap_or(vec![]);

    // Creates a (index, Coin) pair so that it is easier to update this coin later
    let res: Vec<(usize, &Coin)> = coin_vector
        .iter()
        .enumerate()
        .filter(|val| val.1.denom == denom)
        .collect();

    // Token should exist for the given denom
    if res.is_empty() {
        if !add {
            return Err(ContractError::NotFound {
                msg: "Locked tokens don't exist for given denom".to_string(),
            });
        }
        coin_vector.push(Coin {
            amount: amount,
            denom: denom.clone(),
        });
    } else {
        let index = res[0].0;
        let updated_amount: Uint128;
        if add {
            updated_amount = res[0].1.amount + amount;
        } else {
            if res[0].1.amount < amount {
                return Err(ContractError::CustomError {
                    val: "token.amount < amount".into(),
                });
            }

            updated_amount = res[0].1.amount - amount;
        }

        coin_vector[index].amount = updated_amount;
    }

    LOCKED.save(storage, owner, &coin_vector)?;

    Ok(())
}

/// Handles the withdrawal of tokens after completion of locking period.
pub fn handle_withdraw(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    denom: String,
    amount: u64,
    locking_period: LockingPeriod,
) -> Result<Response, ContractError> {
    // Load the token
    let mut vtokens = VTOKENS.load(deps.storage, (info.sender.clone(), &denom))?;

    // Retrive the tokens with the given locking period
    let vtoken: Vec<(usize, &Vtoken)> = vtokens
        .iter()
        .enumerate()
        .filter(|s| s.1.period == locking_period)
        .collect();

    if vtoken.is_empty() {
        return Err(ContractError::NotFound {
            msg: "No token for given locking period".into(),
        });
    }

    // If the tokens haven't unlocked then return Err
    if let Status::Locked = vtoken[0].1.status {
        if vtoken[0].1.end_time > env.block.time {
            return Err(ContractError::NotUnlocked {});
        }
    }

    // Withdraw amount can't be greater unlocked
    if vtoken[0].1.token.amount < Uint128::from(amount) {
        return Err(ContractError::InsufficientFunds {
            funds: vtoken[0].1.token.amount.u128(),
        });
    }

    let index = vtoken[0].0;
    let vtoken = vtoken[0].1.to_owned();

    let PeriodWeight { weight, period: _ } =
        get_period(STATE.load(deps.as_ref().storage)?, locking_period.clone())?;

    // balance post withdrawal
    let token_balance = vtoken.token.amount.sub(Uint128::from(amount));
    let vtoken_balance = vtoken.vtoken.amount.sub(Uint128::from(amount) * weight);
    // Update token balance
    if token_balance.is_zero() {
        vtokens.remove(index);
    } else {
        vtokens[index].token.amount = token_balance;
        vtokens[index].vtoken.amount = vtoken_balance;
        vtokens[index].status = Status::Unlocked;
    }
    // Save the changes to VTOKENS
    if vtokens.len() == 0 {
        VTOKENS.remove(deps.storage, (info.sender.clone(), &info.funds[0].denom));
    } else {
        VTOKENS.save(
            deps.storage,
            (info.sender.clone(), &info.funds[0].denom),
            &vtokens,
        )?;
    }

    // Update LOCKED
    // It is possible that there are no locked tokens for the given owner
    let denom_locked_map = LOCKED.may_load(deps.as_ref().storage, info.sender.clone())?;
    if let Some(mut locked_map) = denom_locked_map {
        let locked_token: Vec<(usize, &Coin)> = locked_map
            .iter()
            .enumerate()
            .filter(|el| el.1.denom == denom)
            .collect();

        // If locked tokens are present for the given denom only then update
        if !locked_token.is_empty() {
            let index = locked_token[0].0;
            locked_map[index].amount -= vtoken.token.amount;
            if locked_map[index].amount.is_zero() {
                locked_map.remove(index);
            }
            LOCKED.save(deps.storage, info.sender.clone(), &locked_map)?;
        }
    };

    // Update UNLOCKED
    // It is possible that the UNLOCKED map hasn't been updated as yet, even though
    // the tokens have completed locking_period
    let unlocked_map = UNLOCKED.may_load(deps.as_ref().storage, info.sender.clone())?;
    match unlocked_map {
        Some(mut unlocked_tokens) => {
            let unlocked_token: Vec<(usize, &Coin)> = unlocked_tokens
                .iter()
                .enumerate()
                .filter(|el| el.1.denom == denom)
                .collect();

            // if denom is present in UNLOCKED, then update the value else if
            // the withdrawn amount is not zero, then create a new entry
            if !unlocked_token.is_empty() {
                let index = unlocked_token[0].0;
                unlocked_tokens[index].amount -= Uint128::from(amount);
                if unlocked_tokens[index].amount.is_zero() {
                    unlocked_tokens.remove(index);
                }
                UNLOCKED.save(deps.storage, info.sender.clone(), &unlocked_tokens)?;
            } else if !token_balance.is_zero() {
                UNLOCKED.save(
                    deps.storage,
                    info.sender.clone(),
                    &vec![Coin {
                        amount: token_balance,
                        denom: denom.clone(),
                    }],
                )?;
            }
        }
        None => {
            // Create a new entry in UNLOCKED only if the whole amount is not
            // withdrawn.
            if !token_balance.is_zero() {
                UNLOCKED.save(
                    deps.storage,
                    info.sender.clone(),
                    &vec![Coin {
                        amount: token_balance,
                        denom: denom.clone(),
                    }],
                )?;
            }
        }
    }

    // Update nft
    let mut nft = TOKENS.load(deps.as_ref().storage, info.sender.clone())?;
    let denom_index: Vec<(usize, &Vtoken)> = nft
        .vtokens
        .iter()
        .enumerate()
        .filter(|el| el.1.token.denom == denom && el.1.period == locking_period.clone())
        .collect();

    let index = denom_index[0].0;
    if token_balance.is_zero() {
        nft.vtokens.remove(index);
    } else {
        nft.vtokens[index].token.amount = token_balance;
        nft.vtokens[index].vtoken.amount = vtoken_balance;
        nft.vtokens[index].status = Status::Unlocked;
    }
    TOKENS.save(deps.storage, info.sender.clone(), &nft)?;

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom,
                amount: Uint128::from(amount),
            }],
        })
        .add_attribute("action", "Withdraw")
        .add_attribute("Recipent", info.sender))
}

fn get_period(state: State, locking_period: LockingPeriod) -> Result<PeriodWeight, ContractError> {
    Ok(match locking_period {
        LockingPeriod::T1 => state.t1,
        LockingPeriod::T2 => state.t2,
        LockingPeriod::T3 => state.t3,
        LockingPeriod::T4 => state.t4,
    })
}

// !------- Only update LOCKED/UNLOCKED according to the vtoken status -------!
/// Handles the transfer of vtokens between users
pub fn handle_transfer_ownership(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    recipient: String,
    locking_period: LockingPeriod,
    denom: String,
    calltype: Option<CallType>,
) -> Result<Response, ContractError> {
    let recipient = deps.api.addr_validate(&recipient)?;

    // Load the denom that needs to be transfered
    let mut sender_vtokens = VTOKENS.load(deps.storage, (info.sender.clone(), &denom))?;

    let sender_vtoken: Vec<(usize, &Vtoken)> = sender_vtokens
        .iter()
        .enumerate()
        .filter(|s| s.1.period == locking_period)
        .collect();

    if sender_vtoken.is_empty() {
        return Err(ContractError::NotFound {
            msg: "No tokens found for the given denom".into(),
        });
    }
    let sender_vtoken_index = sender_vtoken[0].0;
    let sender_vtoken_owned = sender_vtoken[0].1.to_owned();

    // Load the recipients nft
    let recipient_nft = TOKENS.may_load(deps.as_ref().storage, recipient.clone())?;

    let mut recipient_nft = if let Some(val) = recipient_nft {
        val
    } else {
        let mut state = STATE.load(deps.as_ref().storage)?;
        state.num_tokens += 1;
        STATE.save(deps.storage, &state)?;
        TokenInfo {
            owner: recipient.clone(),
            vtokens: vec![],
            token_id: state.num_tokens,
        }
    };

    // Load the recipients vtoken for given denom
    let receiver_vtokens_status = VTOKENS.may_load(deps.storage, (recipient.clone(), &denom))?;

    match receiver_vtokens_status {
        Some(mut receiver_vtokens) => {
            let receiver_vtoken: Vec<(usize, &Vtoken)> = receiver_vtokens
                .iter()
                .enumerate()
                .filter(|s| s.1.period == locking_period)
                .collect();

            // Create new vtoken if not already present, else update
            if receiver_vtoken.is_empty() {
                let new_vtoken = Vtoken {
                    ..sender_vtoken_owned.clone()
                };
                receiver_vtokens.push(new_vtoken.clone());
                recipient_nft.vtokens.push(new_vtoken);
            } else {
                let index = receiver_vtoken[0].0;
                receiver_vtokens[index].token.amount += sender_vtoken_owned.token.amount;
                receiver_vtokens[index].vtoken.amount += sender_vtoken_owned.vtoken.amount;

                // Check if the recipient nft has a vtoken for the given denom,
                // then update else push new
                let recipient_nft_vtoken: Vec<(usize, &Vtoken)> = recipient_nft
                    .vtokens
                    .iter()
                    .enumerate()
                    .filter(|el| el.1.token.denom == denom && el.1.period == locking_period)
                    .collect();

                if recipient_nft_vtoken.is_empty() {
                    recipient_nft.vtokens.push(sender_vtoken_owned.clone());
                } else {
                    let index = recipient_nft_vtoken[0].0;
                    recipient_nft.vtokens[index].token.amount += sender_vtoken_owned.token.amount;
                    recipient_nft.vtokens[index].vtoken.amount += sender_vtoken_owned.vtoken.amount;
                }
            };

            VTOKENS.save(deps.storage, (recipient.clone(), &denom), &receiver_vtokens)?;
            TOKENS.save(deps.storage, recipient.clone(), &recipient_nft)?;
        }
        None => {
            // Create a new vtoken
            let new_vtoken = Vtoken {
                ..sender_vtoken_owned.clone()
            };
            // Append to NFT. Don't need to check if nft has the given denom
            // because VTOKENS and NFT.vtokens are supposed to have the same data.
            // If not present in VTOKENS, then also not in TOKENS.
            recipient_nft.vtokens.push(new_vtoken.clone());

            VTOKENS.save(deps.storage, (recipient.clone(), &denom), &vec![new_vtoken])?;
            TOKENS.save(deps.storage, recipient.clone(), &recipient_nft)?;
        }
    }
    sender_vtokens.remove(sender_vtoken_index);
    VTOKENS.save(deps.storage, (info.sender.clone(), &denom), &sender_vtokens)?;

    // Update Locked
    // Remove the transfered tokens for the senders LOCKED mapping
    let locked_sender_coins_status = LOCKED.may_load(deps.as_ref().storage, info.sender.clone())?;

    if let Some(mut locked_sender_coins) = locked_sender_coins_status {
        let locked_sender_coin: Vec<(usize, &Coin)> = locked_sender_coins
            .iter()
            .enumerate()
            .filter(|el| el.1.denom == denom)
            .collect();

        let index = locked_sender_coin[0].0;
        locked_sender_coins[index].amount -= sender_vtoken_owned.token.amount;

        LOCKED.save(deps.storage, info.sender.clone(), &locked_sender_coins)?;
    }

    // Update Unlocked
    let mut unlocked_sender_coins_status =
        UNLOCKED.may_load(deps.as_ref().storage, info.sender.clone())?;

    if let Some(mut unlocked_sender_coins) = unlocked_sender_coins_status {
        let unlocked_sender_coin: Vec<(usize, &Coin)> = unlocked_sender_coins
            .iter()
            .enumerate()
            .filter(|el| el.1.denom == denom)
            .collect();

        let index = unlocked_sender_coin[0].0;
        unlocked_sender_coins[index].amount -= sender_vtoken_owned.token.amount;

        UNLOCKED.save(deps.storage, info.sender.clone(), &unlocked_sender_coins)?;
    }

    // Update sender nft
    let mut sender_nft = TOKENS.load(deps.as_ref().storage, info.sender.clone())?;

    let sender_nft_denom_vtoken: Vec<(usize, &Vtoken)> = sender_nft
        .vtokens
        .iter()
        .enumerate()
        .filter(|el| el.1.token.denom == denom && el.1.period == locking_period)
        .collect();

    let index = sender_nft_denom_vtoken[0].0;
    sender_nft.vtokens.remove(index);

    TOKENS.save(deps.storage, info.sender.clone(), &sender_nft)?;

    Ok(Response::new()
        .add_attribute("action", "transfer")
        .add_attribute("from", info.sender)
        .add_attribute("to", recipient))
}

pub fn bribe_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
    extended_pair: u64,
) -> Result<Response, ContractError> {
    //check if active proposal
    let proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time < env.block.time.seconds() {
        return Err(ContractError::CustomError {
            val: "Proposal Bribing Period Ended".to_string(),
        });
    }
    // bribe denom should be a single coin
    if info.funds.is_empty() {
        return Err(ContractError::InsufficientFunds { funds: 0 });
    } else if info.funds.len() > 1 {
        return Err(ContractError::CustomError {
            val: String::from("Multiple denominations are not supported as yet."),
        });
    }

    if info.funds[0].amount.is_zero() {
        return Err(ContractError::InsufficientFunds { funds: 0 });
    }

    let bribe_coin = info.funds[0].clone();
    let mut existing_bribes =
        BRIBES_BY_PROPOSAL.load(deps.storage, (proposal_id, extended_pair))?;
    let mut found = false;
    for mut coin in existing_bribes.clone() {
        if bribe_coin.denom == coin.denom {
            coin.amount += bribe_coin.amount;
            found = true;
        }
    }
    if !found {
        existing_bribes.push(bribe_coin);
    }

    BRIBES_BY_PROPOSAL.save(deps.storage, (proposal_id, extended_pair), &existing_bribes)?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn claim_bribe_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    //check if active proposal
    let proposal = PROPOSAL.load(deps.storage, proposal_id)?;

    if proposal.voting_end_time > env.block.time.seconds() {
        return Err(ContractError::CustomError {
            val: "Proposal still in voting period".to_string(),
        });
    }
    // bribe denom should be a single coin

    let mut vote = VOTERSPROPOSAL.load(deps.storage, (info.sender.clone(), proposal_id))?;

    if vote.bribe_claimed {
        return Err(ContractError::CustomError {
            val: "Bribe Already Claimed".to_string(),
        });
    }
    let total_vote_weight = PROPOSALVOTE
        .load(deps.storage, (proposal.app_id, vote.extended_pair))?
        .u128();
    let total_bribe =
        BRIBES_BY_PROPOSAL.load(deps.storage, (proposal.app_id, vote.extended_pair))?;

    let mut claimable_bribe: Vec<Coin> = vec![];

    for coin in total_bribe.clone() {
        let claimable_amount = (vote.vote_weight / total_vote_weight) * coin.amount.u128();
        let claimable_coin = Coin {
            amount: Uint128::from(claimable_amount),
            denom: coin.denom,
        };
        claimable_bribe.push(claimable_coin);
    }

    vote.bribe_claimed = true;
    VOTERSPROPOSAL.save(deps.storage, (info.sender.clone(), proposal_id), &vote)?;
    Ok(Response::new()
        .add_attribute("method", "voted for proposal")
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: claimable_bribe,
        }))
}

pub fn emission(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    _info: MessageInfo,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    // check if already emission executed
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time > env.block.time.seconds() {
        return Err(ContractError::CustomError {
            val: "Proposal Voting Period not ended to execute emission for the proposal"
                .to_string(),
        });
    }
    let app_id = proposal.app_id;
    //check governance token via app_id
    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    let gov_token_id = app_response.gov_token_id;

    let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
    if gov_token_denom.is_empty() || gov_token_id == 0 {
        return Err(ContractError::CustomError {
            val: "Gov token not found".to_string(),
        });
    }

    let vtokens = SUPPLY.load(deps.storage, &gov_token_denom)?;
    let total_v_token = vtokens.vtoken;
    /////query token_circulatin_supply
    let total_weight = get_token_supply(deps.as_ref(), app_id, gov_token_id)?;
    if total_weight == 0 {
        return Err(ContractError::CustomError {
            val: "Current Circulating Supply is 0".to_string(),
        });
    }

    let state = STATE.load(deps.storage)?;
    let query_msg = QueryMsg::VestedTokens {
        denom: gov_token_denom,
    };
    let query_response: Uint128 = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: state.vesting_contract.to_string(),
        msg: to_binary(&query_msg).unwrap(),
    }))?;
    let circulating_supply = Uint128::from(total_weight) - query_response;
    let percentage_locked =
        Decimal::raw(total_v_token).div(Decimal::raw(circulating_supply.u128() + total_v_token));
    let emission = EMISSION.load(deps.storage, proposal.app_id)?;
    let reward_emision = Uint128::from(emission.rewards_pending) * emission.emmission_rate;
    let effective_emission = reward_emision.mul(Decimal::one() - percentage_locked);
    // mint and distribue to vault owner  based vote portion
    let ext_pair = proposal.extended_pair.clone();
    let mut votes: Vec<Uint128> = vec![];
    for i in 0..ext_pair.len() {
        let mut vote = PROPOSALVOTE
            .load(deps.storage, (app_id, ext_pair[i]))
            .unwrap_or_default();
        votes.push(vote);
    }

    proposal.emission_completed = true;
    // bribe denom should be a single coin
    proposal.emission_distributed = effective_emission.u128();
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn rebase(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    // check emission already compluted and executed
    if !proposal.emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission caluclation did not take place to initiate rebase calculation"
                .to_string(),
        });
    }
    if proposal.voting_end_time > env.block.time.seconds() {
        return Err(ContractError::CustomError {
            val: "proposal in voting period".to_string(),
        });
    }
    let app_id = proposal.app_id;
    //check governance token via app_id
    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    let gov_token_id = app_response.gov_token_id;

    let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
    if gov_token_denom.is_empty() || gov_token_id == 0 {
        return Err(ContractError::CustomError {
            val: "Invalid gov token".to_string(),
        });
    }

    let total_weight = get_token_supply(deps.as_ref(), app_id, gov_token_id)?;
    if total_weight == 0 {
        return Err(ContractError::CustomError {
            val: "Current Circulating Supply is 0".to_string(),
        });
    }

    let state = STATE.load(deps.storage)?;
    let query_msg = QueryMsg::VestedTokens {
        denom: gov_token_denom.clone(),
    };
    let query_response: Uint128 = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: state.vesting_contract.to_string(),
        msg: to_binary(&query_msg).unwrap(),
    }))?;
    let circulating_supply = Uint128::from(total_weight) - query_response;

    // calculate rebase amount
    let vtokens = SUPPLY.load(deps.storage, &gov_token_denom)?;
    let total_v_token = vtokens.vtoken;
    let percentage_locked = (Decimal::raw(total_v_token)
        .div(Decimal::raw(circulating_supply.u128())))
    .checked_pow(3)
    .unwrap();

    let rebase_amount = Decimal::percent(50)
        .mul(percentage_locked)
        .mul(Uint128::new(proposal.emission_distributed))
        .u128();
    proposal.rebase_distributed = rebase_amount;

    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

// pub fn claimrebase(
//     deps: DepsMut<ComdexQuery>,
//     env: Env,
//     info: MessageInfo,
//     proposal_id: u64,
// ) -> Result<Response, ContractError> {
//     //check if active proposal
//     let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
//     // check emission already compluted and executed
//     if !proposal.rebase_completed {
//         return Err(ContractError::CustomError {
//             val: "Rebase calculation".to_string(),
//         });
//     }
//     if proposal.voting_end_time > env.block.time.seconds() {
//         return Err(ContractError::CustomError {
//             val: "proposal in voting period".to_string(),
//         });
//     }

//     let app_id = proposal.app_id;
//     //check governance token via app_id
//     let app_response = query_app_exists(deps.as_ref(), app_id)?;

//     let gov_token_id = app_response.gov_token_id;

//     let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
//     if gov_token_denom.is_empty() || gov_token_id == 0 {
//         return Err(ContractError::CustomError {
//             val: "Invalid gov token".to_string(),
//         });
//     }

//     let total_weight = get_token_supply(deps.as_ref(), app_id, gov_token_id)?;
//     if total_weight == 0 {
//         return Err(ContractError::CustomError {
//             val: "Current Circulating Supply is 0".to_string(),
//         });
//     }
//     let total_vtoken_weight = SUPPLY.load(deps.storage, &gov_token_denom)?;
//     let rebase_amount = proposal.rebase_distributed;
//     let v_token_balance = VTOKENS
//         .load(deps.storage, (info.sender, &gov_token_denom))?
//         .vtoken
//         .amount;
//     let rebase_claimable = (v_token_balance.u128().div(total_vtoken_weight.vtoken)) * rebase_amount;

//     PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
//     Ok(Response::new().add_attribute("method", "voted for proposal"))
// }

// pub fn vote_proposal(
//     deps: DepsMut<ComdexQuery>,
//     env: Env,
//     info: MessageInfo,
//     app_id: u64,
//     proposal_id: u64,
//     extended_pair: u64,
// ) -> Result<Response, ContractError> {
//     let has_voted = VOTERS_VOTE
//         .load(deps.storage, (info.sender.clone(), proposal_id))
//         .unwrap_or_default();
//     if has_voted {
//         return Err(ContractError::CustomError {
//             val: "Already voted for the proposal".to_string(),
//         });
//     }
//     //check if active proposal
//     let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
//     if proposal.voting_end_time < env.block.time.seconds() {
//         return Err(ContractError::CustomError {
//             val: "Proposal Voting Period Ended".to_string(),
//         });
//     }
//     let app_response = query_app_exists(deps.as_ref(), app_id)?;

//     let extended_pairs = proposal.extended_pair.clone();
//     let mut found_pair = false;
//     for i in 0..extended_pairs.len() {
//         if extended_pairs[i] == extended_pair {
//             found_pair = true;
//         }
//     }
//     if !found_pair {
//         return Err(ContractError::CustomError {
//             val: "Invalid Extended pair".to_string(),
//         });
//     }
//     //balance of owner for the for denom for voting

//     let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;
//     if gov_token_denom.is_empty() || app_response.gov_token_id == 0 {
//         return Err(ContractError::CustomError {
//             val: "Gov token not found for the app".to_string(),
//         });
//     }
//     let vote_power = VTOKENS.load(deps.storage, (info.sender.clone(), &gov_token_denom))?;
//     // get token owner balance
//     proposal.total_voted_weight += vote_power.vtoken.amount.u128();
//     let proposal_vote = PROPOSALVOTE
//         .load(deps.storage, (proposal_id, extended_pair))
//         .unwrap_or_default();
//     PROPOSALVOTE.save(
//         deps.storage,
//         (proposal_id, extended_pair),
//         &(vote_power.vtoken.amount + proposal_vote),
//     )?;
//     PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
//     let vote = Vote {
//         app_id: app_id,
//         extended_pair: extended_pair,
//         vote_weight: vote_power.vtoken.amount.u128(),
//         bribe_claimed: false,
//     };
//     VOTERSPROPOSAL.save(deps.storage, (info.sender, proposal_id), &vote)?;

//     Ok(Response::new().add_attribute("method", "voted for proposal"))
// }

pub fn raise_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    _info: MessageInfo,
    app_id: u64,
) -> Result<Response, ContractError> {
    //check if app exist
    query_app_exists(deps.as_ref(), app_id)?;
    //get ext pairs vec from app
    //need binding
    let response = vec![1, 2, 3];

    //check no proposal active for app
    let current_app_proposal = match APPCURRENTPROPOSAL.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => 0,
    };

    if current_app_proposal != 0 {
        let proposal = PROPOSAL.load(deps.storage, current_app_proposal)?;
        if proposal.voting_end_time > env.block.time.seconds() {
            return Err(ContractError::CustomError {
                val: "Previous proposal in voting state for the app".to_string(),
            });
        }
    }

    // set proposal data
    let voting_period = VOTINGPERIOD.load(deps.storage).unwrap_or_default();
    //update proposal maps
    let proposal = Proposal {
        app_id: app_id,
        voting_start_time: env.block.time.seconds(),
        voting_end_time: env.block.time.seconds() + voting_period,
        extended_pair: response,
        emission_completed: false,
        rebase_completed: false,
        emission_distributed: 0,
        rebase_distributed: 0,
        total_voted_weight: 0,
    };
    let current_proposal = PROPOSALCOUNT.load(deps.storage).unwrap_or_default();
    PROPOSALCOUNT.save(deps.storage, &(current_proposal + 1))?;
    APPCURRENTPROPOSAL.save(deps.storage, app_id, &(current_proposal + 1))?;
    PROPOSAL.save(deps.storage, current_proposal + 1, &proposal)?;
    Ok(Response::new().add_attribute("method", "reset"))
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{coin, coins, Addr, Api, OwnedDeps, Querier, StdError};

    const DENOM: &str = "TKN";

    /// Returns default InstantiateMsg with each value in seconds.
    /// - t1 is 1 week (7*24*60*60), similarly, t2 is 2 weeks, t3 is 3 weeks
    /// and t4 is 4 weeks.
    /// - unlock_period is 1 week
    fn init_msg() -> InstantiateMsg {
        InstantiateMsg {
            t1: PeriodWeight {
                period: 604_800,
                weight: Decimal::from_atomics(Uint128::new(25), 2).unwrap(),
            },
            t2: PeriodWeight {
                period: 1_209_600,
                weight: Decimal::from_atomics(Uint128::new(50), 2).unwrap(),
            },
            t3: PeriodWeight {
                period: 1_814_400,
                weight: Decimal::from_atomics(Uint128::new(75), 2).unwrap(),
            },
            t4: PeriodWeight {
                period: 2_419_200,
                weight: Decimal::from_atomics(Uint128::new(100), 2).unwrap(),
            },
            voting_period: 604_800,
            vesting_contract: Addr::unchecked("vesting_contract"),
        }
    }

    fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, ComdexQuery> {
        OwnedDeps {
            storage: MockStorage::default(),
            api: MockApi::default(),
            querier: MockQuerier::default(),
            custom_query_type: PhantomData,
        }
    }
    #[test]
    fn proper_initialization() {
        let env = mock_env();
        let mut deps = mock_dependencies();
        let info = mock_info("sender", &coins(0, DENOM.to_string()));

        let msg = init_msg();
        assert_eq!(msg.t1.weight.to_string(), "0.25");

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes.len(), 2);

        let state = STATE.load(&deps.storage).unwrap();
        assert_eq!(state.t1, msg.t1);
        assert_eq!(state.t3, msg.t3);
    }

    #[test]
    fn lock_create_new_nft() {
        // mock values
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &coins(0, DENOM.to_string()));

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
            calltype: None,
        };

        // Successful execution
        let info = mock_info("user1", &coins(100, DENOM.to_string()));

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes.len(), 2);

        let sender_addr = Addr::unchecked("user1");
        let token = TOKENS.load(&deps.storage, sender_addr.clone()).unwrap();

        assert_eq!(token.owner, sender_addr.clone());
        assert_eq!(token.token_id, 1u64);
        assert_eq!(token.vtokens.len(), 1);
        // .token should be the same as locked tokens
        assert_eq!(
            token.vtokens[0].token,
            Coin {
                amount: Uint128::from(100u32),
                denom: DENOM.to_string()
            }
        );
        // .vtoken should be correct Vtoken released
        assert_eq!(
            token.vtokens[0].vtoken,
            Coin {
                amount: Uint128::from(25u32),
                denom: String::from("vTKN")
            }
        );
        assert_eq!(token.vtokens[0].start_time, env.block.time);
        assert_eq!(
            token.vtokens[0].end_time,
            env.block.time.plus_seconds(imsg.t1.period)
        );
        assert_eq!(token.vtokens[0].period, LockingPeriod::T1);
        assert_eq!(token.vtokens[0].status, Status::Locked);

        // Check to see the LOCKED token mapping has correctly changed
        let locked_tokens = LOCKED
            .load(deps.as_ref().storage, sender_addr.clone())
            .unwrap();
        assert_eq!(locked_tokens.len(), 1);
        assert_eq!(locked_tokens[0].amount.u128(), 100u128);
        assert_eq!(locked_tokens[0].denom, DENOM.to_string());

        // Check to see the SUPPLY mapping is correct
        let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
        assert_eq!(supply.token, 100u128);
        assert_eq!(supply.vtoken, 25u128);
    }

    #[test]
    fn lock_different_denom_and_time_period() {
        // mock values
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &coins(0, DENOM.to_string()));

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        let info = mock_info("owner", &coins(100, "DNM1".to_string()));
        let owner_addr = Addr::unchecked("owner");

        // Create a new entry for DENOM in nft
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("owner", &coins(100, "DNM2".to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T2,
            None,
        )
        .unwrap();

        // Check correct update in TOKENS
        let nft = TOKENS
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(nft.vtokens.len(), 2);
        assert_eq!(nft.vtokens[0].token.denom, "DNM1".to_string());
        assert_eq!(nft.vtokens[0].vtoken.denom, "vDNM1".to_string());
        assert_eq!(nft.vtokens[0].token.amount.u128(), 100u128);
        assert_eq!(nft.vtokens[0].vtoken.amount.u128(), 25u128);
        assert_eq!(nft.vtokens[0].start_time, env.block.time);
        assert_eq!(
            nft.vtokens[0].end_time,
            env.block.time.plus_seconds(imsg.t1.period)
        );
        assert_eq!(nft.vtokens[0].period, LockingPeriod::T1);
        assert_eq!(nft.vtokens[0].status, Status::Locked);

        assert_eq!(nft.vtokens[1].token.denom, "DNM2".to_string());
        assert_eq!(nft.vtokens[1].vtoken.denom, "vDNM2".to_string());
        assert_eq!(nft.vtokens[1].token.amount.u128(), 100u128);
        assert_eq!(nft.vtokens[1].vtoken.amount.u128(), 50u128);
        assert_eq!(nft.vtokens[1].start_time, env.block.time);
        assert_eq!(
            nft.vtokens[1].end_time,
            env.block.time.plus_seconds(imsg.t2.period)
        );
        assert_eq!(nft.vtokens[1].period, LockingPeriod::T2);
        assert_eq!(nft.vtokens[1].status, Status::Locked);

        // Check correct update in LOCKED
        let locked_map = LOCKED
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(locked_map.len(), 2);
        assert_eq!(locked_map[0].denom, "DNM1".to_string());
        assert_eq!(locked_map[0].amount.u128(), 100u128);
        assert_eq!(locked_map[1].denom, "DNM2".to_string());
        assert_eq!(locked_map[1].amount.u128(), 100u128);

        // Check correct update in SUPPLY
        let supply = SUPPLY.load(deps.as_ref().storage, "DNM1").unwrap();
        assert_eq!(supply.token, 100u128);
        assert_eq!(supply.vtoken, 25u128);

        let supply = SUPPLY.load(deps.as_ref().storage, "DNM2").unwrap();
        assert_eq!(supply.token, 100u128);
        assert_eq!(supply.vtoken, 50u128);
    }

    #[test]
    fn lock_same_denom_and_time_period() {
        // mock values
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let owner_addr = Addr::unchecked("owner");

        // Create a new entry for DENOM in nft
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // forward the time, inside 1 week
        env.block.time = env.block.time.plus_seconds(100_000);

        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T1,
            Some(CallType::UpdatePeriod),
        )
        .unwrap();

        // Check correct updation in nft
        let nft = TOKENS
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(nft.vtokens.len(), 1);
        assert_eq!(nft.vtokens[0].token.amount.u128(), 200u128);
        assert_eq!(nft.vtokens[0].vtoken.amount.u128(), 50u128);
        assert_eq!(nft.vtokens[0].start_time, env.block.time);
        assert_eq!(
            nft.vtokens[0].end_time,
            env.block.time.plus_seconds(imsg.t1.period)
        );
        assert_eq!(nft.vtokens[0].period, LockingPeriod::T1);
        assert_eq!(nft.vtokens[0].status, Status::Locked);

        // Check correct updation in LOCKED
        let locked_tokens = LOCKED
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(locked_tokens.len(), 1);
        assert_eq!(locked_tokens[0].amount.u128(), 200u128);
        assert_eq!(locked_tokens[0].denom, DENOM.to_string());

        // Check correct update in SUPPLY
        let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
        assert_eq!(supply.vtoken, 50u128);
        assert_eq!(supply.token, 200u128);
    }

    #[test]
    fn lock_same_denom_diff_time_period() {
        // mock values
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &coins(0, DENOM.to_string()));

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let owner_addr = Addr::unchecked("owner");

        // Create a new entry for DENOM in nft
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // forward the time, inside 1 week
        env.block.time = env.block.time.plus_seconds(100_000);

        // Lock for new time period
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T2,
            None,
        )
        .unwrap();

        // Check correct updation in nft
        let nft = TOKENS
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(nft.vtokens.len(), 2);
        assert_eq!(nft.vtokens[0].token.amount.u128(), 100u128);
        assert_eq!(nft.vtokens[0].vtoken.amount.u128(), 25u128);
        assert_eq!(nft.vtokens[1].token.amount.u128(), 100u128);
        assert_eq!(nft.vtokens[1].vtoken.amount.u128(), 50u128);
        assert_eq!(nft.vtokens[1].start_time, env.block.time);
        assert_eq!(
            nft.vtokens[1].end_time,
            env.block.time.plus_seconds(imsg.t2.period)
        );
        assert_eq!(nft.vtokens[0].period, LockingPeriod::T1);
        assert_eq!(nft.vtokens[0].status, Status::Locked);
        assert_eq!(nft.vtokens[1].period, LockingPeriod::T2);
        assert_eq!(nft.vtokens[1].status, Status::Locked);

        // Check correct updation in LOCKED
        let locked_tokens = LOCKED
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(locked_tokens.len(), 1);
        assert_eq!(locked_tokens[0].amount.u128(), 200u128);
        assert_eq!(locked_tokens[0].denom, DENOM.to_string());

        // Check correct update in SUPPLY
        let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
        assert_eq!(supply.token, 200u128);
        assert_eq!(supply.vtoken, 75u128);
    }

    #[test]
    fn lock_zero_transfer() {
        // mock values
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &coins(0, DENOM.to_string()));

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // This should throw an error because the amount is zero
        let res = handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T1,
            None,
        )
        .unwrap_err();
        match res {
            ContractError::InsufficientFunds { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn withdraw_basic_functionality() {
        // mock values
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
            calltype: None,
        };

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        // Check correct update in LOCKED
        let locked_tokens = LOCKED.load(deps.as_ref().storage, owner.clone()).unwrap();
        assert_eq!(locked_tokens.len(), 1);
        assert_eq!(locked_tokens[0].amount.u128(), 100u128);
        assert_eq!(locked_tokens[0].denom, DENOM.to_string());

        env.block.time = env.block.time.plus_seconds(imsg.t1.period + 1u64);

        // Withdrawing 10 Tokens
        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            info.funds[0].denom.clone(),
            10,
            LockingPeriod::T1,
        )
        .unwrap();
        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes.len(), 2);
        assert_eq!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![coin(10, DENOM.to_string())]
            })
        );

        // Check correct update in LOCKED
        // Entry from LOCKED should be removed, since the tokens have completed
        // their locking period
        let locked = LOCKED.load(deps.as_ref().storage, owner.clone()).unwrap();
        assert_eq!(locked.len(), 0);

        // Check correct update in UNLOCKED
        let unlocked = UNLOCKED.load(deps.as_ref().storage, owner.clone()).unwrap();
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].denom, DENOM.to_string());
        assert_eq!(unlocked[0].amount.u128(), 90u128);

        // Just a check if the order matters with Decimal
        let vtoken_balance1 = Uint128::from(90u64) * imsg.t1.weight;
        let vtoken_balance2 = imsg.t1.weight * Uint128::from(90u64);
        assert_eq!(vtoken_balance1, vtoken_balance2);

        // Check correct update in VTOKENS
        // Should left 100 - 10 = 90 tokens
        let vtoken = VTOKENS
            .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
            .unwrap();
        assert_eq!(vtoken.len(), 1);
        assert_eq!(vtoken[0].token.amount.u128(), 90u128);
        let vtoken_balance = Uint128::from(25u64).sub(Uint128::from(10u64) * imsg.t1.weight);
        assert_eq!(vtoken[0].vtoken.amount.u128(), vtoken_balance.u128());
        assert_eq!(vtoken[0].status, Status::Unlocked);

        // Check correct update in nft
        let nft = TOKENS.load(deps.as_ref().storage, owner.clone()).unwrap();
        assert_eq!(nft.vtokens.len(), 1);
        assert_eq!(nft.vtokens[0].token.amount.u128(), 90u128);
        assert_eq!(nft.vtokens[0].vtoken.amount.u128(), vtoken_balance.u128());
        assert_eq!(nft.vtokens[0].status, Status::Unlocked);

        // Withdrawing All Tokens and Should remove the vtoken.
        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            info.funds[0].denom.clone(),
            90,
            LockingPeriod::T1,
        )
        .unwrap();
        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes.len(), 2);
        assert_eq!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![coin(90, DENOM.to_string())]
            })
        );

        let res = VTOKENS
            .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
            .unwrap_err();
        match res {
            StdError::NotFound { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn withdraw_no_vtokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            DENOM.to_string(),
            100u64,
            LockingPeriod::T1,
        )
        .unwrap_err();
        match res {
            ContractError::Std(StdError::NotFound { .. }) => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn withdraw_not_unlocked() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
            calltype: None,
        };

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let owner = Addr::unchecked("onwer");

        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            DENOM.to_string(),
            120,
            LockingPeriod::T1,
        )
        .unwrap_err();
        match res {
            ContractError::NotUnlocked { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn withdraw_period_not_locked() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
            calltype: None,
        };

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let owner = Addr::unchecked("onwer");

        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            DENOM.to_string(),
            10,
            LockingPeriod::T2,
        )
        .unwrap_err();
        match res {
            ContractError::NotFound { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn withdraw_denom_not_locked() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
            calltype: None,
        };

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let owner = Addr::unchecked("onwer");

        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            "DNM1".to_string(),
            10,
            LockingPeriod::T1,
        )
        .unwrap_err();
        match res {
            ContractError::Std(StdError::NotFound { .. }) => {}
            e => panic!("{:?}", e),
        };
    }
}
