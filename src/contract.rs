use crate::error::ContractError;
use crate::helpers::{
    get_token_supply, query_app_exists, query_extended_pair_by_app, query_get_asset_data,
    query_pool_by_app, query_surplus_reward, query_whitelisted_asset,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SudoMsg};
use crate::state::{
    Delegation, DelegationInfo, DelegationStats, EmissionVaultPool, Proposal, UserDelegationInfo,
    Vote, VotePair, ADMIN, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, COMPLETEDPROPOSALS, CSWAP_ID,
    DELEGATED, DELEGATION_INFO, DELEGATION_STATS, EMISSION, EMISSION_REWARD, MAXPROPOSALCLAIMED,
    PROPOSAL, PROPOSALCOUNT, PROPOSALVOTE, REBASE_CLAIMED, VOTERSPROPOSAL, VOTERS_CLAIM,
    VOTERS_CLAIMED_PROPOSALS, VOTERS_VOTE,
};
use crate::state::{
    LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, STATE, SUPPLY,
    TOKENS, VTOKENS,
};

use comdex_bindings::{ComdexMessages, ComdexQuery};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, Api, BankMsg, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, QueryRequest,
    Response, StdError, StdResult, Storage, Uint128, WasmQuery,
};
use cw2::set_contract_version;
use std::ops::{Div, Mul};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:locking_contract";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    // Validate recieved addresses
    deps.api
        .addr_validate(&msg.vesting_contract.clone().into_string())?;
    deps.api.addr_validate(&msg.admin.clone().into_string())?;
    map_validate(deps.api, &msg.foundation_addr)?;

    //query_app_exists(deps.as_ref(), msg.emission.app_id)?;
    let mut foundation_addr_unique = msg.foundation_addr.clone();
    foundation_addr_unique.sort_unstable();
    foundation_addr_unique.dedup();

    let state = State {
        t1: msg.t1,
        t2: msg.t2,
        num_tokens: 0_u64,
        vesting_contract: msg.vesting_contract,
        foundation_addr: foundation_addr_unique,
        foundation_percentage: msg.foundation_percentage,
        surplus_asset_id: msg.surplus_asset_id,
        voting_period: msg.voting_period,
        min_lock_amount: msg.min_lock_amount,
    };

    if msg.foundation_percentage > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Foundation Emission percentage cannot be greater than 100 %".to_string(),
        });
    }
    if msg.emission.rewards_pending == 0 {
        return Err(ContractError::CustomError {
            val: "Pending rewards should not be zero %".to_string(),
        });
    }
    if msg.emission.distributed_rewards != 0 {
        return Err(ContractError::CustomError {
            val: "Distributed rewards should be zero".to_string(),
        });
    }

    if msg.emission.emission_rate > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Emission rate cannot be greater one".to_string(),
        });
    }

    // Set Contract version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Set State
    STATE.save(deps.storage, &state)?;
    EMISSION.save(deps.storage, msg.emission.app_id, &msg.emission)?;
    PROPOSALCOUNT.save(deps.storage, &0)?;
    CSWAP_ID.save(deps.storage, &msg.cswap_id)?;
    ADMIN.set(deps, Some(msg.admin))?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

pub fn map_validate(api: &dyn Api, admins: &[String]) -> StdResult<Vec<Addr>> {
    admins.iter().map(|addr| api.addr_validate(addr)).collect()
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<ComdexMessages>, ContractError> {
    match msg {
        ExecuteMsg::VoteProposal {
            app_id,
            proposal_id,
            extended_pair,
            ratio,
        } => {
            //check if app exist
            let app_response = query_app_exists(deps.as_ref(), app_id)?;

            //// get gov token denom for app
            let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;

            ////check if gov token exist
            if gov_token_denom.is_empty() || app_response.gov_token_id == 0 {
                return Err(ContractError::CustomError {
                    val: "Gov token not found for the app".to_string(),
                });
            }
            vote_proposal(
                deps,
                env,
                info,
                app_id,
                proposal_id,
                extended_pair,
                gov_token_denom,
                ratio,
            )
        }
        ExecuteMsg::RaiseProposal { app_id } => {
            //check if app exist
            query_app_exists(deps.as_ref(), app_id)?;

            let cswap_id = CSWAP_ID.load(deps.storage)?;

            let mut pools = query_pool_by_app(deps.as_ref(), cswap_id)?;

            for i in pools.iter_mut() {
                *i += 1000000;
            }

            ////get ext pairs vec from app
            let mut ext_pairs = query_extended_pair_by_app(deps.as_ref(), app_id)?;
            ext_pairs.append(&mut pools);

            raise_proposal(deps, env, info, app_id, ext_pairs)
        }
        ExecuteMsg::Bribe {
            proposal_id,
            extended_pair,
        } => {
            // CHECK IF BRIBE ASSET EXISTS ON-CHAIN
            let bribe_coin = info.funds[0].clone();
            let found = query_whitelisted_asset(deps.as_ref(), bribe_coin.denom.clone())?;
            if !found {
                return Err(ContractError::CustomError {
                    val: String::from("Asset not whitelisted on chain"),
                });
            }

            bribe_proposal(deps, env, info, proposal_id, extended_pair, bribe_coin)
        }
        ExecuteMsg::ClaimReward {
            app_id,
            proposal_id,
        } => claim_rewards(deps, env, info, app_id, proposal_id),
        ExecuteMsg::Emission { proposal_id } => emission(deps, env, info, proposal_id),
        ExecuteMsg::Lock {
            app_id,
            locking_period,
            recipient,
        } => {
            let app_response = query_app_exists(deps.as_ref(), app_id)?;
            let gov_token_id = app_response.gov_token_id;
            let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
            if gov_token_denom.is_empty() || gov_token_id == 0 {
                return Err(ContractError::CustomError {
                    val: "Gov token not found".to_string(),
                });
            }

            if info.funds[0].denom != gov_token_denom {
                return Err(ContractError::CustomError {
                    val: "Wrong Deposit token".to_string(),
                });
            }
            handle_lock_nft(deps, env, info, app_id, locking_period, recipient)
        }
        ExecuteMsg::Withdraw { denom } => handle_withdraw(deps, env, info, denom),
        // ExecuteMsg::Transfer {
        //     recipient,
        //     locking_period,
        //     denom,
        // } => handle_transfer(deps, env, info, recipient, locking_period, denom),
        ExecuteMsg::Rebase { proposal_id } => calculate_rebase_reward(deps, env, info, proposal_id),
        ExecuteMsg::Delegate {
            delegation_address,
            denom,
            ratio,
        } => delegate(deps, env, info, delegation_address, denom, ratio),
        ExecuteMsg::Undelegate {
            delegation_address,
            denom,
        } => undelegate(deps, env, info, delegation_address, denom),
        ExecuteMsg::UpdateProtocolFees {
            delegate_address,
            fees,
        } => update_protocol_fees(deps, env, info, delegate_address, fees),
        _ => Err(ContractError::CustomError {
            val: "Invalid message".to_string(),
        }),
    }
}

//// delegation function/////

pub fn delegate(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    delegation_address: Addr,
    denom: String,
    ratio: Decimal,
) -> Result<Response<ComdexMessages>, ContractError> {
    //// check if delegation_address exists////
    let delegation_info = DELEGATION_INFO.may_load(deps.storage, delegation_address.clone())?;
    if delegation_info.is_none() {
        return Err(ContractError::CustomError {
            val: "Delegation info not found".to_string(),
        });
    }

    ///// check if sender is not delegated
    if info.sender.clone() == delegation_address {
        return Err(ContractError::CustomError {
            val: "Sender is not allowed to self-delegated".to_string(),
        });
    }

    ///// get voting power
    // balance of owner for the for denom for voting

    let vtokens = VTOKENS.may_load_at_height(
        deps.storage,
        (info.sender.clone(), &denom),
        env.block.height,
    )?;

    if vtokens.is_none() {
        return Err(ContractError::CustomError {
            val: "No tokens locked to perform voting on proposals".to_string(),
        });
    }

    let vtokens = vtokens.unwrap();
    // calculate voting power for the the proposal
    let mut vote_power: u128 = 0;

    for vtoken in vtokens {
        vote_power += vtoken.vtoken.amount.u128();
    }
    let delegation_stats = DELEGATION_STATS.may_load(deps.storage, delegation_address.clone())?;
    let total_delegated_amount = ratio.mul(Uint128::from(vote_power)).u128();
    let delegation = DELEGATED.may_load(deps.storage, info.sender.clone())?;
    if delegation.is_none() {
        //create new delegation
        let mut delegation = UserDelegationInfo {
            total_casted: total_delegated_amount,
            delegations: vec![],
        };
        let mut delegations = delegation.delegations;
        let delegation_new = Delegation {
            delegated_to: delegation_address.clone(),
            delegated_at: env.block.time,
            delegation_end_at: env.block.time.plus_seconds(86400), ///////set as 1 day
            delegated: total_delegated_amount,
        };
        delegations.push(delegation_new);
        delegation.delegations = delegations;
        DELEGATED.save(
            deps.storage,
            info.sender.clone(),
            &delegation,
            env.block.height,
        )?;

        if delegation_stats.is_none() {
            let delegation_stats = DelegationStats {
                total_delegated: total_delegated_amount,
                total_delegators: 1,
            };
            DELEGATION_STATS.save(
                deps.storage,
                delegation_address.clone(),
                &delegation_stats,
                env.block.height,
            )?;
        } else {
            let mut delegation_stats = delegation_stats.unwrap();
            delegation_stats.total_delegated += total_delegated_amount;
            DELEGATION_STATS.save(
                deps.storage,
                delegation_address.clone(),
                &delegation_stats,
                env.block.height,
            )?;
        }
    } else {
        let mut delegation = delegation.unwrap();
        let mut delegations = delegation.delegations;
        let mut prev_delegation: u128 = 0;
        let mut found = false;
        for delegation_tmp in delegations.iter_mut() {
            if delegation_address == delegation_tmp.delegated_to {
                prev_delegation = delegation_tmp.delegated;
                delegation_tmp.delegated = total_delegated_amount;
                delegation_tmp.delegated_at = env.block.time;
                delegation_tmp.delegation_end_at = env.block.time.plus_seconds(86400); ///////set as 1 day
                found = true;
                break;
            } else {
                continue;
            }
        }

        if found {
            let mut delegation_stats = delegation_stats.unwrap();
            delegation_stats.total_delegated -= prev_delegation;
            delegation_stats.total_delegated -= total_delegated_amount;
            DELEGATION_STATS.save(
                deps.storage,
                delegation_address.clone(),
                &delegation_stats,
                env.block.height,
            )?;

            delegation.total_casted =
                delegation.total_casted - prev_delegation + total_delegated_amount;
            delegation.delegations = delegations;
            DELEGATED.save(
                deps.storage,
                info.sender.clone(),
                &delegation,
                env.block.height,
            )?;
        } else {
            let mut delegation_stats = delegation_stats.unwrap();
            delegation_stats.total_delegated -= total_delegated_amount;
            DELEGATION_STATS.save(
                deps.storage,
                delegation_address.clone(),
                &delegation_stats,
                env.block.height,
            )?;

            let delegation_new = Delegation {
                delegated_to: delegation_address.clone(),
                delegated_at: env.block.time,
                delegation_end_at: env.block.time.plus_seconds(86400), ///////set as 1 day
                delegated: total_delegated_amount,
            };
            delegations.push(delegation_new);
            delegation.delegations = delegations;
            DELEGATED.save(
                deps.storage,
                info.sender.clone(),
                &delegation,
                env.block.height,
            )?;
        }
    }

    Ok(Response::new()
        .add_attribute("action", "delegate")
        .add_attribute("from", info.sender)
        .add_attribute("delegated_address", delegation_address))
}

pub fn undelegate(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    delegation_address: Addr,
    denom: String,
) -> Result<Response<ComdexMessages>, ContractError> {
    //// check if delegation_address exists////
    let delegation_info = DELEGATION_INFO.may_load(deps.storage, delegation_address.clone())?;
    if delegation_info.is_none() {
        return Err(ContractError::CustomError {
            val: "Emission calculation did not take place to initiate foundation calculation"
                .to_string(),
        });
    }

    ////// check if delegation is present ///////
    let delegation = DELEGATED.may_load(deps.storage, info.sender.clone())?;
    if delegation.is_none() {
        return Err(ContractError::CustomError {
            val: "No active delegation present to undelegate".to_string(),
        });
    }

    //// check if UnDelegation time has reached
    let mut delegation_info = delegation.unwrap();
    let mut delegations = delegation_info.delegations;

    let delegations_len = delegations.len();

    for i in 0..delegations_len {
        if delegations[i].delegated_to == delegation_address {
            if delegations[i].delegation_end_at > env.block.time {
                return Err(ContractError::CustomError {
                    val: "Yet to reach UnDelegation time".to_string(),
                });
            }
        } else {
            delegations.remove(i);
            delegation_info.delegations = delegations;
            DELEGATED.save(
                deps.storage,
                info.sender.clone(),
                &delegation_info,
                env.block.height,
            )?;
            break;
        }
    }

    Ok(Response::new()
        .add_attribute("action", "undelegate")
        .add_attribute("from", info.sender)
        .add_attribute("delegated_address", delegation_address))
}

pub fn emission_foundation(
    deps: DepsMut<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Vec<ComdexMessages>, ContractError> {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    // check emission already computed and executed
    if !proposal.emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission calculation did not take place to initiate foundation calculation"
                .to_string(),
        });
    }

    if !info.funds.is_empty() {
        return Err(ContractError::CustomError {
            val: "Funds not accepted".to_string(),
        });
    }

    //check if foundation emission has not taken place for the proposal
    if proposal.foundation_emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission already distributed".to_string(),
        });
    }

    let state = STATE.load(deps.storage)?;

    let foundation_addr = state.foundation_addr;

    if foundation_addr.is_empty() {
        return Err(ContractError::CustomError {
            val: "No foundation address found".to_string(),
        });
    }
    let foundation_emission = proposal.foundation_distributed;

    //// message to send tokens to foundation_address
    let emission_msg = ComdexMessages::MsgFoundationEmission {
        app_id: proposal.app_id,
        amount: Uint128::from(foundation_emission),
        foundation_address: foundation_addr,
    };
    proposal.foundation_emission_completed = true;

    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;

    Ok(vec![emission_msg])
}

fn lock_funds(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    _app_id: u64,
    sender: Addr,
    funds: Coin,
    locking_period: LockingPeriod,
) -> Result<(), ContractError> {
    // Load internal state containing locking period details.
    let mut state = STATE.load(deps.storage)?;
    if state.min_lock_amount > funds.amount {
        return Err(ContractError::CustomError {
            val: "Lock amount less than minimum lock amount".to_string(),
        });
    }

    // Load the locking period and weight
    let PeriodWeight { period, weight } = get_period(state.clone(), locking_period.clone())?;

    // Create a new Vtoken
    let new_vtoken = create_vtoken(
        deps.storage,
        env.clone(),
        locking_period,
        period,
        weight,
        funds.clone(),
    )?;

    // Loads the NFT, if present.
    let nft = TOKENS.may_load(deps.storage, sender.clone())?;

    match nft {
        // NFT already exists
        Some(_) => {}

        // Create a new NFT
        None => {
            state.num_tokens += 1;

            let new_nft = TokenInfo {
                owner: sender.clone(),
                token_id: state.num_tokens,
            };

            STATE.save(deps.storage, &state)?;

            TOKENS.save(deps.storage, sender.clone(), &new_nft)?;
        }
    };

    // Update VTOKENS
    VTOKENS.update(
        deps.storage,
        (sender, &funds.denom),
        env.block.height,
        |el| -> StdResult<Vec<Vtoken>> {
            // If value exists for given key, then update else create.
            match el {
                Some(mut val) => {
                    val.push(new_vtoken);
                    Ok(val)
                }

                None => Ok(vec![new_vtoken]),
            }
        },
    )?;

    Ok(())
}

/// Lock the sent tokens and create corresponding vtokens
pub fn handle_lock_nft(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    locking_period: LockingPeriod,
    recipient: Option<Addr>,
) -> Result<Response<ComdexMessages>, ContractError> {
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
    if let Some(recipient_address) = recipient {
        deps.api.addr_validate(recipient_address.as_str())?;
        lock_funds(
            deps,
            env,
            app_id,
            recipient_address,
            info.funds[0].clone(),
            locking_period,
        )?;
    } else {
        lock_funds(
            deps,
            env,
            app_id,
            info.sender.clone(),
            info.funds[0].clone(),
            locking_period,
        )?;
    }

    Ok(Response::new()
        .add_attribute("action", "lock")
        .add_attribute("from", info.sender))
}

/// Create a new Vtoken with the given period, weight, funds.
fn create_vtoken(
    storage: &mut dyn Storage,
    env: Env,
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
        env.clone(),
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

/// Update the SUPPLY map for the total supply for vtokens and the corresponding
/// tokens locked.
fn update_denom_supply(
    storage: &mut dyn Storage,
    env: Env,
    denom: &str,
    vquantity: u128,
    quantity: u128,
    add: bool,
) -> Result<(), ContractError> {
    // Load the total supply in the for the given denom
    let denom_supply = SUPPLY.may_load(storage, denom)?;
    if denom_supply.is_none() && !add {
        return Err(ContractError::NotFound {
            msg: "vTokens don't exist for the given denom".to_string(),
        });
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
            return Err(ContractError::InsufficientFunds {
                funds: denom_supply_struct.vtoken,
            });
        } else if denom_supply_struct.token < quantity {
            return Err(ContractError::InsufficientFunds {
                funds: denom_supply_struct.token,
            });
        }

        denom_supply_struct.vtoken -= vquantity;
        denom_supply_struct.token -= quantity;
    }

    SUPPLY.save(storage, denom, &denom_supply_struct, env.block.height)?;

    Ok(())
}

/// Handles the withdrawal of tokens after completion of locking period.
pub fn handle_withdraw(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    denom: String,
) -> Result<Response<ComdexMessages>, ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    // Load the token
    let vtokens = VTOKENS.may_load(deps.storage, (info.sender.clone(), &denom))?;
    if vtokens.is_none() {
        return Err(ContractError::NotFound {
            msg: format!("No tokens found for {:?}", denom),
        });
    }

    let mut vtokens_denom = vtokens.unwrap();

    let mut vote_power: u128 = 0;

    for vtoken in vtokens_denom.iter() {
        vote_power += vtoken.vtoken.amount.u128();
    }

    // Retrieve unlocked tokens with the given locking period
    let vtokens: Vec<(usize, &Vtoken)> = vtokens_denom
        .iter()
        .enumerate()
        .filter(|s| s.1.end_time < env.block.time)
        .collect();

    // No unlocked tokens
    if vtokens.is_empty() {
        return Err(ContractError::NotFound {
            msg: format!("No unlocked tokens found for {:?}", denom),
        });
    }

    // Calculate total withdrawable amount and remove the corresponding VToken
    let mut withdrawable = 0u128;

    let mut vwithdrawable = 0u128;
    let mut indices: Vec<usize> = vec![];
    for (index, vtoken) in vtokens {
        withdrawable += vtoken.token.amount.u128();
        vwithdrawable += vtoken.vtoken.amount.u128();
        indices.push(index);
    }
    for index in indices.into_iter().rev() {
        vtokens_denom.remove(index);
    }

    let mut delegation = DELEGATED
        .may_load(deps.storage, info.sender.clone())?
        .unwrap();
    let total_delegated = delegation.total_casted;

    if total_delegated < vote_power - vwithdrawable {
    } else {
        for delegation_temp in delegation.delegations.iter_mut() {
            let rhs = Decimal::from_ratio(vote_power - vwithdrawable, total_delegated);
            let temp = delegation_temp.delegated;
            let mut delegation_stats = DELEGATION_STATS
                .may_load(deps.storage, delegation_temp.delegated_to.clone())?
                .unwrap();
            delegation_stats.total_delegated = delegation_stats.total_delegated - temp;
            delegation_stats.total_delegated = delegation_stats.total_delegated
                + rhs.mul(Uint128::new(delegation_temp.delegated)).u128();

            delegation_temp.delegated = rhs.mul(Uint128::new(delegation_temp.delegated)).u128();
            DELEGATION_STATS.save(
                deps.storage,
                delegation_temp.delegated_to.clone(),
                &delegation_stats,
                env.block.height,
            )?;
        }
        delegation.total_casted = delegation.total_casted - vote_power - vwithdrawable;
        DELEGATED.save(
            deps.storage,
            info.sender.clone(),
            &delegation,
            env.block.height,
        )?;
    }
    // Update VTOKENS
    if vtokens_denom.is_empty() {
        VTOKENS.remove(
            deps.storage,
            (info.sender.clone(), &denom),
            env.block.height,
        )?;
    } else {
        VTOKENS.save(
            deps.storage,
            (info.sender.clone(), &denom),
            &vtokens_denom,
            env.block.height,
        )?;
    };

    // Reduce the total supply
    update_denom_supply(
        deps.storage,
        env.clone(),
        &denom,
        vwithdrawable,
        withdrawable,
        false,
    )?;

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom,
                amount: Uint128::from(withdrawable),
            }],
        })
        .add_attribute("action", "Withdraw")
        .add_attribute("from", info.sender))
}

/// Given the locking period, retrieves the `period` and `weight`.
fn get_period(state: State, locking_period: LockingPeriod) -> Result<PeriodWeight, ContractError> {
    Ok(match locking_period {
        LockingPeriod::T1 => state.t1,
        LockingPeriod::T2 => state.t2,
    })
}

/// Handles the transfer of vtokens between users
pub fn handle_transfer(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    recipient: String,
    locking_period: LockingPeriod,
    denom: String,
) -> Result<Response<ComdexMessages>, ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    let recipient = deps.api.addr_validate(&recipient)?;

    // Load the sender denom that needs to be transferred
    let sender_vtokens = VTOKENS.may_load(deps.storage, (info.sender.clone(), &denom))?;

    if sender_vtokens.is_none() {
        return Err(ContractError::NotFound {
            msg: format!("No tokens found for {:?}", denom),
        });
    }
    let sender_denom_vtokens = sender_vtokens.unwrap();

    // Load tokens with given locking period
    let sender_vtokens_to_transfer: Vec<&Vtoken> = sender_denom_vtokens
        .iter()
        .filter(|s| s.period == locking_period)
        .collect();

    if sender_vtokens_to_transfer.is_empty() {
        return Err(ContractError::NotFound {
            msg: format!(
                "No tokens found for denom: {:?} and locking period: {:?}",
                denom, locking_period
            ),
        });
    }

    {
        // Load the recipients VTOKENS
        let recipient_vtokens =
            VTOKENS.may_load(deps.as_ref().storage, (recipient.clone(), &denom))?;

        let mut recipient_vtokens = if let Some(val) = recipient_vtokens {
            val
        } else {
            vec![]
        };

        // Extend the recipient vtokens with the sender vtokens
        for vtoken in sender_vtokens_to_transfer {
            recipient_vtokens.push(vtoken.to_owned())
        }

        VTOKENS.save(
            deps.storage,
            (recipient.clone(), &denom),
            &recipient_vtokens,
            env.block.height,
        )?;
    }

    {
        // Remaining vtokens are saved to sender's VTOKENS
        let sender_vtokens_remaining: Vec<Vtoken> = sender_denom_vtokens
            .into_iter()
            .filter(|el| !(el.period == locking_period))
            .collect();

        if sender_vtokens_remaining.is_empty() {
            VTOKENS.remove(
                deps.storage,
                (info.sender.clone(), &denom),
                env.block.height,
            )?;
        } else {
            VTOKENS.save(
                deps.storage,
                (info.sender.clone(), &denom),
                &sender_vtokens_remaining,
                env.block.height,
            )?;
        }
    }

    // Load the recipients nft
    let recipient_nft = TOKENS.may_load(deps.as_ref().storage, recipient.clone())?;

    match recipient_nft {
        Some(_) => {}
        None => {
            // Crate a new NFT
            let mut state = STATE.load(deps.as_ref().storage)?;
            state.num_tokens += 1;
            STATE.save(deps.storage, &state)?;
            TOKENS.save(
                deps.storage,
                recipient.clone(),
                &TokenInfo {
                    owner: recipient.clone(),
                    token_id: state.num_tokens,
                },
            )?;
        }
    };

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
    bribe_coin: Coin,
) -> Result<Response<ComdexMessages>, ContractError> {
    // bribe denom should be a single coin
    if info.funds.is_empty() {
        return Err(ContractError::InsufficientFunds { funds: 0 });
    } else if info.funds.len() > 1 {
        return Err(ContractError::CustomError {
            val: String::from("Multiple denominations are not supported as yet."),
        });
    }

    // bribe coin should not have zero amount
    if info.funds[0].amount.is_zero() {
        return Err(ContractError::InsufficientFunds { funds: 0 });
    }

    //check if active proposal
    let proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time < env.block.time {
        return Err(ContractError::CustomError {
            val: "Proposal Bribing Period Ended".to_string(),
        });
    }

    // check if ext_pair param exist in extended pair list to vote for

    let extended_pairs = proposal.extended_pair;

    match extended_pairs.binary_search(&extended_pair) {
        Ok(_) => (),
        Err(_) => {
            return Err(ContractError::CustomError {
                val: "Invalid Extended pair".to_string(),
            })
        }
    }

    // UPDATE BRIBE FOR PROPOSAL (IF EXISTS THEN UPDATE ELSE APPEND)
    let mut existing_bribes =
        match BRIBES_BY_PROPOSAL.may_load(deps.storage, (proposal_id, extended_pair))? {
            Some(record) => record,
            None => vec![],
        };

    if !existing_bribes.is_empty() {
        let mut found = false;
        for coin1 in existing_bribes.iter_mut() {
            if bribe_coin.denom == coin1.denom {
                coin1.amount += bribe_coin.amount;
                found = true;
            }
        }
        if !found {
            existing_bribes.push(bribe_coin);
        }
    } else {
        existing_bribes = vec![bribe_coin];
    }

    BRIBES_BY_PROPOSAL.save(deps.storage, (proposal_id, extended_pair), &existing_bribes)?;
    Ok(Response::new().add_attribute("method", "bribe"))
}

pub fn claim_rewards(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    proposal_id: Option<u64>,
) -> Result<Response<ComdexMessages>, ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    let all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };
    let mut bribe_coins = vec![];
    if proposal_id.is_some() {
        if !all_proposals.contains(&proposal_id.unwrap()) {
            return Err(ContractError::CustomError {
                val: String::from("proposal not completed"),
            });
        }

        let voters_claimed = VOTERS_CLAIM
            .load(deps.storage, (info.sender.clone(), proposal_id.unwrap()))
            .unwrap_or_default();

        if voters_claimed {
            return Err(ContractError::CustomError {
                val: String::from("Already claimed"),
            });
        }

        bribe_coins = calculate_bribe_reward_proposal(
            deps.as_ref(),
            env.clone(),
            info.clone(),
            proposal_id.unwrap(),
        )?;
        VOTERS_CLAIM.save(
            deps.storage,
            (info.sender.clone(), proposal_id.unwrap()),
            &true,
        )?;
        let mut claimed_proposal =
            match VOTERS_CLAIMED_PROPOSALS.may_load(deps.storage, info.sender.clone())? {
                Some(val) => val,
                None => vec![],
            };
        claimed_proposal.push(proposal_id.unwrap());
        claimed_proposal.sort();
        VOTERS_CLAIMED_PROPOSALS.save(deps.storage, info.sender.clone(), &claimed_proposal)?;
        bribe_coins.sort_by_key(|element| element.denom.clone());
    } else {
        let (_bribe_coins, claimed_proposal) = calculate_bribe_reward(
            deps.as_ref(),
            env.clone(),
            info.clone(),
            all_proposals.clone(),
            app_id,
        )?;

        bribe_coins = _bribe_coins;
        MAXPROPOSALCLAIMED.save(
            deps.storage,
            (app_id, info.sender.clone()),
            all_proposals.last().unwrap(),
        )?;
        VOTERS_CLAIMED_PROPOSALS.save(deps.storage, info.sender.clone(), &claimed_proposal)?;
        bribe_coins.sort_by_key(|element| element.denom.clone());
        for proposal in claimed_proposal {
            VOTERS_CLAIM.save(deps.storage, (info.sender.clone(), proposal), &true)?;
        }
    }
    if !bribe_coins.is_empty() {
        Ok(Response::new()
            .add_attribute("method", "External Incentive Claimed")
            .add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: bribe_coins,
            }))
    } else {
        Err(ContractError::CustomError {
            val: String::from("No rewards to claim."),
        })
    }
}

pub fn calculate_bribe_reward(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    all_proposals: Vec<u64>,
    _app_id: u64,
) -> Result<(Vec<Coin>, Vec<u64>), ContractError> {
    let mut bribe_coins: Vec<Coin> = vec![];
    let mut claimed_proposal =
        match VOTERS_CLAIMED_PROPOSALS.may_load(deps.storage, info.sender.clone())? {
            Some(val) => val,
            None => vec![],
        };
    for proposalid in all_proposals {
        let voters_claimed = VOTERS_CLAIM
            .load(deps.storage, (info.sender.clone(), proposalid))
            .unwrap_or_default();

        if voters_claimed {
            continue;
        }
        let vote = match VOTERSPROPOSAL.may_load(deps.storage, (info.sender.clone(), proposalid))? {
            Some(val) => val,
            None => continue,
        };

        for pair in vote.votes {
            let total_vote_weight = PROPOSALVOTE
                .load(deps.storage, (proposalid, pair.extended_pair))?
                .u128();

            let total_bribe = match BRIBES_BY_PROPOSAL
                .may_load(deps.storage, (proposalid, pair.extended_pair))?
            {
                Some(val) => val,
                None => vec![],
            };

            let mut claimable_bribe: Vec<Coin> = vec![];
            for coin in total_bribe.clone() {
                let claimable_amount = (Decimal::new(Uint128::from(pair.vote_weight))
                    .div(Decimal::new(Uint128::from(total_vote_weight))))
                .mul(coin.amount);
                let claimable_coin = Coin {
                    amount: claimable_amount,
                    denom: coin.denom,
                };
                claimable_bribe.push(claimable_coin);
            }

            for bribe_deposited in claimable_bribe.clone() {
                match bribe_coins
                    .iter_mut()
                    .find(|p| bribe_deposited.denom == p.denom)
                {
                    Some(pivot) => {
                        pivot.denom = bribe_deposited.denom;
                        pivot.amount += bribe_deposited.amount;
                    }
                    None => {
                        bribe_coins.push(bribe_deposited);
                    }
                }
            }
        }
        claimed_proposal.push(proposalid);
        claimed_proposal.sort();
    }

    //// send bank message to band

    Ok((bribe_coins, claimed_proposal))
}

pub fn calculate_bribe_reward_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Vec<Coin>, ContractError> {
    let mut bribe_coins: Vec<Coin> = vec![];

    let _vote = VOTERSPROPOSAL.may_load(deps.storage, (info.sender.clone(), proposal_id))?;

    if _vote.is_some() {
        let vote = _vote.unwrap();
        for pair in vote.votes {
            let total_vote_weight = PROPOSALVOTE
                .load(deps.storage, (proposal_id, pair.extended_pair))?
                .u128();

            let total_bribe = match BRIBES_BY_PROPOSAL
                .may_load(deps.storage, (proposal_id, pair.extended_pair))?
            {
                Some(val) => val,
                None => vec![],
            };

            let mut claimable_bribe: Vec<Coin> = vec![];
            for coin in total_bribe.clone() {
                let claimable_amount = (Decimal::new(Uint128::from(pair.vote_weight))
                    .div(Decimal::new(Uint128::from(total_vote_weight))))
                .mul(coin.amount);
                let claimable_coin = Coin {
                    amount: claimable_amount,
                    denom: coin.denom,
                };
                claimable_bribe.push(claimable_coin);
            }

            for bribe_deposited in claimable_bribe.clone() {
                match bribe_coins
                    .iter_mut()
                    .find(|p| bribe_deposited.denom == p.denom)
                {
                    Some(pivot) => {
                        pivot.denom = bribe_deposited.denom;
                        pivot.amount += bribe_deposited.amount;
                    }
                    None => {
                        bribe_coins.push(bribe_deposited);
                    }
                }
            }
        }
    }
    Ok(bribe_coins)
}

pub fn calculate_rebase_reward(
    mut deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    let proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    if !proposal.emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission for proposal not completed".to_string(),
        });
    }
    let has_rebased = REBASE_CLAIMED
        .load(deps.storage, (info.sender.clone(), proposal_id))
        .unwrap_or_default();

    if has_rebased {
        return Err(ContractError::CustomError {
            val: "Already claimed rebase".to_string(),
        });
    }
    let total_rebase_amount: u128 = proposal.rebase_distributed;

    let app_response = query_app_exists(deps.as_ref(), proposal.app_id)?;

    let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;

    //// get v-tokens at proposal height
    let vtokens = match VTOKENS.may_load_at_height(
        deps.storage,
        (info.sender.clone(), &gov_token_denom),
        proposal.height,
    )? {
        Some(val) => val,
        None => vec![],
    };
    if vtokens.is_empty() {
        return Err(ContractError::CustomError {
            val: "No locked tokens for users to claim rebase".to_string(),
        });
    }
    let supply = SUPPLY
        .may_load_at_height(deps.storage, &gov_token_denom, proposal.height)?
        .unwrap();
    let total_locked: u128 = supply.token;
    //// get rebase amount per period
    let mut locked_t1: u128 = 0;
    let mut locked_t2: u128 = 0;

    for vtoken in vtokens {
        match vtoken.period {
            LockingPeriod::T1 => locked_t1 += vtoken.token.amount.u128(),
            LockingPeriod::T2 => locked_t2 += vtoken.token.amount.u128(),
        }
    }

    //// lock in t1
    let lock_amount_t1 = Uint128::from(locked_t1).mul(Decimal::from_ratio(
        Uint128::from(total_rebase_amount),
        Uint128::from(total_locked),
    ));

    if lock_amount_t1 != Uint128::zero() {
        let fund_t1 = Coin {
            amount: lock_amount_t1,
            denom: gov_token_denom.clone(),
        };

        lock_funds(
            deps.branch(),
            env.clone(),
            proposal.app_id,
            info.sender.clone(),
            fund_t1,
            LockingPeriod::T1,
        )?;
    }
    let lock_amount_t2 = Uint128::from(locked_t2).mul(Decimal::from_ratio(
        Uint128::from(total_rebase_amount),
        Uint128::from(total_locked),
    ));

    if lock_amount_t2 != Uint128::zero() {
        let fund_t2 = Coin {
            amount: lock_amount_t2,
            denom: gov_token_denom,
        };
        lock_funds(
            deps.branch(),
            env,
            proposal.app_id,
            info.sender.clone(),
            fund_t2,
            LockingPeriod::T2,
        )?;
    }

    if lock_amount_t1 == Uint128::zero() && lock_amount_t2 == Uint128::zero() {
        return Err(ContractError::CustomError {
            val: "Claimable rebase ratio not met for the existing locks".to_string(),
        });
    }
    REBASE_CLAIMED.save(deps.storage, (info.sender, proposal_id), &true)?;
    Ok(Response::new().add_attribute("method", "rebase all holders"))
}

pub fn calculate_surplus_reward(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    max_proposal_claimed: u64,
    all_proposals: Vec<u64>,
    app_id: u64,
) -> Result<Coin, ContractError> {
    let mut asset_denom = String::new();
    let mut total_claimable: u128 = 0_u128;
    for proposalid in all_proposals {
        if proposalid <= max_proposal_claimed {
            continue;
        }
        let proposal = PROPOSAL.load(deps.storage, proposalid)?;
        if proposal.total_surplus.denom.eq("nodenom") {
            continue;
        }
        let proposal_surplus = proposal.total_surplus.amount;
        asset_denom = proposal.total_surplus.denom;

        let app_response = query_app_exists(deps, app_id)?;
        let gov_token_denom = query_get_asset_data(deps, app_response.gov_token_id)?;

        let vtokens = VTOKENS.may_load_at_height(
            deps.storage,
            (info.sender.clone(), &gov_token_denom),
            proposal.height,
        )?;
        if vtokens.is_none() {
            continue;
        }

        let supply = SUPPLY
            .may_load_at_height(deps.storage, &gov_token_denom, proposal.height)?
            .unwrap();
        let total_locked: u128 = supply.vtoken;
        //// get rebase amount per period
        let mut locked: u128 = 0;
        let vtokens = vtokens.unwrap();
        for vtoken in vtokens {
            locked += vtoken.vtoken.amount.u128();
        }
        let mut share = Decimal::new(Uint128::from(locked))
            .div(Decimal::new(Uint128::from(total_locked)))
            .mul(Uint128::from(1_u8))
            .u128();
        share *= proposal_surplus.u128();
        total_claimable += share;
    }
    let claim_coin = Coin {
        amount: Uint128::from(total_claimable),
        denom: asset_denom,
    };

    Ok(claim_coin)
}

pub fn emission(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    //// Only admin can execute
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    // do not accept funds
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }

    // check if already emission executed

    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission already completed".to_string(),
        });
    }
    //check if active proposal
    if proposal.voting_end_time > env.block.time {
        return Err(ContractError::CustomError {
            val: "Proposal Voting Period not ended to execute emission for the proposal"
                .to_string(),
        });
    }

    //check governance token via app_id
    let app_id = proposal.app_id;
    let app_response = query_app_exists(deps.as_ref(), app_id)?;
    let gov_token_id = app_response.gov_token_id;
    let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
    if gov_token_denom.is_empty() || gov_token_id == 0 {
        return Err(ContractError::CustomError {
            val: "Gov token not found".to_string(),
        });
    }

    //// GET TOTAL V-TOKEN SUPPLY
    let vtokens = SUPPLY
        .may_load_at_height(deps.storage, &gov_token_denom, proposal.height)?
        .unwrap();
    let total_v_token = vtokens.vtoken;
    /////query token TOTAL SUPPLY
    let total_weight = get_token_supply(deps.as_ref(), app_id, gov_token_id)?;
    if total_weight == 0 {
        return Err(ContractError::CustomError {
            val: "Current Circulating Supply is 0".to_string(),
        });
    }

    //// GET TOTAL VESTED TOKEN
    let state = STATE.load(deps.storage)?;
    let query_msg = QueryMsg::VestedTokens {
        denom: gov_token_denom,
    };
    let query_response: Uint128 = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: state.vesting_contract.to_string(),
        msg: to_binary(&query_msg).unwrap(),
    }))?;

    //// CALCULATE CIRCULATING SUPPLY (TOTAL SUPPLY-VESTED TOKENS-vtokens(locked))
    let circulating_supply =
        Uint128::from(total_weight) - query_response - Uint128::from(vtokens.token);

    //// QUERY PERCENTAGE LOCKED (VTOKENS_LOCKED/V-TOKENS+CIRCULATING SUPPLY)
    let percentage_locked =
        Decimal::raw(total_v_token).div(Decimal::raw(circulating_supply.u128() + total_v_token));

    //// LOAD EMISSION FOR THE APP
    let mut emission = EMISSION.load(deps.storage, proposal.app_id)?;

    //// CALCULATE EFFECTIVE EMISSION = (REWARD_PENDING*EMISSION RATE)*(1-PERCENTAGE_LOCKED)
    let reward_emission = Uint128::from(emission.rewards_pending) * emission.emission_rate;
    let effective_emission = reward_emission.mul(Decimal::one() - percentage_locked);
    // mint and distribute to vault owner  based vote portion
    let ext_pair = proposal.extended_pair.clone();
    let mut votes: Vec<Uint128> = vec![];
    let mut extd_pair: Vec<u64> = vec![];
    let mut votes_pool: Vec<Uint128> = vec![];
    let mut pool_ids: Vec<u64> = vec![];
    let mut total_vote: Uint128 = Uint128::zero();
    for i in ext_pair {
        let vote = PROPOSALVOTE
            .load(deps.storage, (proposal_id, i))
            .unwrap_or_else(|_| Uint128::from(0_u32));
        if Uint128::from(i).div(Uint128::from(1000000_u64)) == Uint128::zero() {
            votes.push(vote);
            extd_pair.push(i);
        } else {
            let pool_id = i % 1000000;
            votes_pool.push(vote);
            pool_ids.push(pool_id);
        }
        total_vote += vote;
    }

    //// UPDATE Foundation Nodes Share
    proposal.foundation_distributed = (state.foundation_percentage.mul(effective_emission)).u128();
    //// Update proposal Emission State
    proposal.emission_completed = true;
    //// effective emission
    proposal.emission_distributed =
        effective_emission.u128() - (state.foundation_percentage.mul(effective_emission)).u128();
    // update effective emission

    //// UPDATE REBASE AMOUNT
    proposal.rebase_distributed = (reward_emission.mul(percentage_locked)).u128();
    //// EMISSION Data Update
    emission.rewards_pending -= reward_emission.u128();
    emission.distributed_rewards += reward_emission.u128();

    let pool_votes: Uint128 = votes_pool.iter().sum();
    let vault_votes: Uint128 = votes.iter().sum();
    let pools_share = pool_votes
        .mul(Uint128::from(proposal.emission_distributed))
        .div(total_vote);
    let vault_share = Uint128::from(proposal.emission_distributed) - pools_share;
    let mut pool_rewards: Vec<Uint128> = vec![];
    let mut vault_rewards: Vec<Uint128> = vec![];
    for votes in votes_pool.iter() {
        if pool_votes.is_zero() {
            break;
        }
        let reward = votes.mul(pools_share).div(pool_votes);
        pool_rewards.push(reward);
    }

    for vote in votes.iter() {
        if vault_votes.is_zero() {
            break;
        }
        let reward = vote.mul(vault_share).div(vault_votes);
        vault_rewards.push(reward);
    }

    let emission_reward = EmissionVaultPool {
        app_id: app_id,
        pool_ids: pool_ids.clone(),
        vault_ids: extd_pair.clone(),
        total_emission_rewards: Uint128::from(proposal.emission_distributed),
        pool_rewards: pool_rewards,
        vault_rewards: vault_rewards,
    };

    EMISSION_REWARD.save(deps.storage, proposal_id, &emission_reward)?;
    let surplus = query_surplus_reward(deps.as_ref(), app_id, state.surplus_asset_id)?;
    proposal.total_surplus = surplus.clone();
    EMISSION.save(deps.storage, proposal.app_id, &emission)?;
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;

    let mut msg: Vec<ComdexMessages> = vec![];
    let app_id_param = app_id;

    let emission_msg = ComdexMessages::MsgEmissionRewards {
        app_id: app_id_param,
        amount: vault_share,
        extended_pair: extd_pair,
        voting_ratio: votes,
    };

    let cswap_id = CSWAP_ID.load(deps.storage)?;
    let emission_msg_pools = ComdexMessages::MsgEmissionPoolRewards {
        app_id: app_id,
        cswap_app_id: cswap_id,
        amount: pools_share,
        pools: pool_ids,
        voting_ratio: votes_pool,
    };

    let rebase_msg = ComdexMessages::MsgRebaseMint {
        app_id: app_id_param,
        amount: Uint128::from(proposal.rebase_distributed),
        contract_addr: env.contract.address.to_string(),
    };
    let surplus_msg = ComdexMessages::MsgGetSurplusFund {
        app_id: app_id_param,
        asset_id: state.surplus_asset_id,
        contract_addr: env.contract.address.clone().into_string(),
        amount: surplus.clone(),
    };

    if total_vote != Uint128::zero() {
        if vault_votes.ne(&Uint128::zero()) {
            msg.push(emission_msg);
        }
        if pool_votes.ne(&Uint128::zero()) {
            msg.push(emission_msg_pools);
        }
    }
    msg.push(rebase_msg);

    if surplus.amount != Uint128::new(0) {
        msg.push(surplus_msg);
    }

    let mut all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };
    all_proposals.push(proposal_id);
    COMPLETEDPROPOSALS.save(deps.storage, app_id, &all_proposals)?;
    let vec_foundation = emission_foundation(deps, env, info, proposal_id)?;
    msg.extend(vec_foundation);
    Ok(Response::new()
        .add_attribute("method", "emission")
        .add_messages(msg))
}

pub fn update_protocol_fees(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    delegation_address: Addr,
    new_delegator_fees: Decimal,
) -> Result<Response<ComdexMessages>, ContractError> {
    //// check if delegation_address exists////
    let delegation_info = DELEGATION_INFO.may_load(deps.storage, delegation_address.clone())?;
    if delegation_info.is_none() {
        return Err(ContractError::CustomError {
            val: "Delegation address does not exist".to_string(),
        });
    }
    // check if the sender is the owner of the delegation
    let mut delegation_info = delegation_info.unwrap();
    if delegation_info.delegated_address != info.sender {
        return Err(ContractError::CustomError {
            val: "Sender is not the owner of the delegation".to_string(),
        });
    }
    if new_delegator_fees > delegation_info.delegator_fees {
        return Err(ContractError::CustomError {
            val: "New Delegator fees cannot be greater than delegator fees".to_string(),
        });
    }
    if new_delegator_fees < Decimal::zero() {
        return Err(ContractError::CustomError {
            val: "New Delegator fees percentage cannot be less than 0 %".to_string(),
        });
    }
    // update the protocol fees
    delegation_info.delegator_fees = new_delegator_fees;
    DELEGATION_INFO.save(
        deps.storage,
        delegation_address,
        &delegation_info,
        env.block.height,
    )?;

    Ok(Response::new()
        .add_attribute("action", "update_protocol_fees")
        .add_attribute("from", info.sender))
}

pub fn vote_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    _app_id: u64,
    proposal_id: u64,
    extended_pair: Vec<u64>,
    gov_token_denom: String,
    ratio: Vec<Decimal>,
) -> Result<Response<ComdexMessages>, ContractError> {
    // check if admin (admin cannot vote)
    if ADMIN.is_admin(deps.as_ref(), &info.sender)? {
        return Err(ContractError::CustomError {
            val: "Admin cannot vote".to_string(),
        });
    }

    // do not accept  funds
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }

    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;

    // Check if proposal in voting period
    if proposal.voting_end_time < env.block.time {
        return Err(ContractError::CustomError {
            val: "Proposal Voting Period Ended".to_string(),
        });
    }

    // check if vote sequence is correct
    if extended_pair.len() != ratio.clone().len() {
        return Err(ContractError::CustomError {
            val: "Invalid ratio".to_string(),
        });
    }

    let mut total_ration = Decimal::zero();
    for ratio in ratio.iter() {
        total_ration = total_ration + ratio;
    }

    //// check if total ratio is 100%
    if total_ration > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Ratio cannot be more than 100 %".to_string(),
        });
    }

    // check if total ratio is not 0%
    if total_ration == Decimal::zero() {
        return Err(ContractError::CustomError {
            val: "voted ratio cannot be zero".to_string(),
        });
    }

    //// check if already voted for proposal
    let has_voted = VOTERS_VOTE
        .may_load(deps.storage, (info.sender.clone(), proposal_id))?
        .unwrap_or_default();

    // check if ext_pair param exist in extended pair list to vote for

    let extended_pairs_proposal = proposal.extended_pair.clone();

    //// check if extended pair exists in proposal's extended pair

    if !extended_pairs_proposal
        .iter()
        .all(|item| extended_pair.contains(item))
    {
        return Err(ContractError::CustomError {
            val: "Extended pair does not exist in proposal".to_string(),
        });
    }

    //balance of owner for the for denom for voting

    let vtokens = VTOKENS.may_load_at_height(
        deps.storage,
        (info.sender.clone(), &gov_token_denom),
        proposal.height,
    )?;

    if vtokens.is_none() {
        return Err(ContractError::CustomError {
            val: "No tokens locked to perform voting on proposals".to_string(),
        });
    }

    let vtokens = vtokens.unwrap();

    // calculate voting power for the the proposal
    let mut vote_power: u128 = 0;

    for vtoken in vtokens {
        vote_power += vtoken.vtoken.amount.u128();
    }

    //// if a delegated_address is voting
    let delegator_locked =
        DELEGATION_STATS.may_load_at_height(deps.storage, info.sender.clone(), proposal.height)?;

    if delegator_locked.is_some() {
        let delegator_locked = delegator_locked.unwrap().total_delegated;
        vote_power += delegator_locked;
    }

    //// decrease voting power if delegated
    let delegation =
        DELEGATED.may_load_at_height(deps.storage, info.sender.clone(), proposal.height)?;
    if delegation.is_some() {
        let delegation = delegation.unwrap();
        vote_power -= delegation.total_casted;
    }

    //if already voted , decrease previous vote weight
    if has_voted {
        let prev_vote = VOTERSPROPOSAL.load(deps.storage, (info.sender.clone(), proposal_id))?;
        let last_vote_weight = prev_vote.votes;
        for pair_vote in last_vote_weight {
            let mut proposal_vote = PROPOSALVOTE
                .load(deps.storage, (proposal_id, pair_vote.extended_pair))
                .unwrap_or_default();
            proposal_vote -= Uint128::from(pair_vote.vote_weight);
            PROPOSALVOTE.save(
                deps.storage,
                (proposal_id, pair_vote.extended_pair),
                &proposal_vote,
            )?;
            proposal.total_voted_weight -= pair_vote.vote_weight;
        }
    }

    let mut vote_pair: Vec<VotePair> = vec![];
    for (i, pair) in extended_pair.iter().enumerate() {
        let vote_pair_param = VotePair {
            extended_pair: *pair,
            vote_ratio: ratio[i],
            vote_weight: Uint128::from(vote_power).mul(ratio[i]).u128(),
        };
        vote_pair.push(vote_pair_param);
    }

    for pair_vote in vote_pair.iter_mut() {
        let mut proposal_vote = PROPOSALVOTE
            .load(deps.storage, (proposal_id, pair_vote.extended_pair))
            .unwrap_or_default();
        proposal_vote += Uint128::from(pair_vote.vote_weight);
        PROPOSALVOTE.save(
            deps.storage,
            (proposal_id, pair_vote.extended_pair),
            &proposal_vote,
        )?;
        proposal.total_voted_weight -= pair_vote.vote_weight;
    }
    let vote = Vote {
        voting_power_total: vote_power,
        total_voted_ratio: total_ration,
        votes: vote_pair,
    };
    VOTERSPROPOSAL.save(deps.storage, (info.sender.clone(), proposal_id), &vote)?;
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    VOTERS_VOTE.save(deps.storage, (info.sender, proposal_id), &true)?;

    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn raise_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    ext_pairs: Vec<u64>,
) -> Result<Response<ComdexMessages>, ContractError> {
    //// only admin can execute
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    // do not accept  funds
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }

    //// No proposal
    if ext_pairs.is_empty() {
        return Err(ContractError::CustomError {
            val: "No extended pair to vote".to_string(),
        });
    }
    //check no proposal active for app
    let current_app_proposal = (APPCURRENTPROPOSAL.may_load(deps.storage, app_id)?).unwrap_or(0);

    // if proposal already exist , check if whether it is in voting period
    // proposal cannot be raised until current proposal voting time is ended
    if current_app_proposal != 0 {
        let proposal = PROPOSAL.load(deps.storage, current_app_proposal)?;
        if proposal.voting_end_time > env.block.time {
            return Err(ContractError::CustomError {
                val: "Previous proposal in voting state for the app".to_string(),
            });
        }
    }

    // set proposal data
    let state = STATE.load(deps.storage)?;
    let voting_period = state.voting_period;
    let app_id_param = app_id;
    //update proposal maps
    let proposal = Proposal {
        app_id: app_id_param,              //app_id for proposal
        voting_start_time: env.block.time, // Current block timestamp
        voting_end_time: env.block.time.plus_seconds(voting_period), // end voting timestamp
        extended_pair: ext_pairs,          // extended pairs for which voting is taking place
        emission_completed: false,         //initially set emission_completed as false
        rebase_completed: false,           //initially set rebase_completed as false
        emission_distributed: 0,           //emission distributed token as 0
        rebase_distributed: 0,             //rebase distributed token as 0
        total_voted_weight: 0,             // total_weight of voted vtoken
        foundation_emission_completed: false, // emission to foundation addresses as false
        foundation_distributed: 0,         // total distributed tokens as 0
        total_surplus: Coin {
            amount: Uint128::from(0_u32),
            denom: "nodenom".to_string(),
        }, // initialized dummy token
        height: env.block.height,          // current block height of token,
    };
    let mut current_proposal = PROPOSALCOUNT.load(deps.storage).unwrap_or(0);
    current_proposal += 1;
    PROPOSALCOUNT.save(deps.storage, &current_proposal)?;
    APPCURRENTPROPOSAL.save(deps.storage, app_id, &current_proposal)?;
    PROPOSAL.save(deps.storage, current_proposal, &proposal)?;
    Ok(Response::new()
        .add_attribute("method", "proposal_raised")
        .add_attribute("proposal_id", current_proposal.to_string()))
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, ContractError> {
    let ver = cw2::get_contract_version(deps.storage)?;
    // ensure we are migrating from an allowed contract
    if ver.contract != CONTRACT_NAME {
        return Err(StdError::generic_err("Can only upgrade from same type").into());
    }
    // note: better to do proper semver compare, but string compare *usually* works
    if ver.version.as_str() > CONTRACT_VERSION {
        return Err(StdError::generic_err("Cannot upgrade from a newer version").into());
    }
    CSWAP_ID.save(deps.storage, &msg.cswap_id)?;
    // set the new version
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    //do any desired state migrations...

    Ok(Response::default())
}

#[entry_point]
pub fn sudo(deps: DepsMut, env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::UpdateVestingContract { address } => {
            let mut state = STATE.load(deps.storage)?;
            state.vesting_contract = deps.api.addr_validate(&address.into_string())?;
            STATE.save(deps.storage, &state)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateEmissionRate {
            emission_rate,
            app_id,
        } => {
            let mut emission = EMISSION.load(deps.storage, app_id)?;
            if emission_rate > Decimal::one() {
                return Err(ContractError::CustomError {
                    val: "Emission rate cannot be greater one".to_string(),
                });
            }
            emission.emission_rate = emission_rate;
            EMISSION.save(deps.storage, emission.app_id, &emission)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateFoundationInfo {
            addresses,
            foundation_percentage,
        } => {
            if foundation_percentage > Decimal::one() {
                return Err(ContractError::CustomError {
                    val: "Foundation Emission percentage cannot be greater than 100 %".to_string(),
                });
            }
            let mut state = STATE.load(deps.storage)?;
            map_validate(deps.api, &addresses)?;
            state.foundation_addr = addresses;
            state.foundation_addr.sort_unstable();
            state.foundation_addr.dedup();
            state.foundation_percentage = foundation_percentage;
            STATE.save(deps.storage, &state)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateLockingPeriod { t1, t2 } => {
            let mut state = STATE.load(deps.storage)?;
            state.t1 = t1;
            state.t2 = t2;
            STATE.save(deps.storage, &state)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateAdmin { admin } => {
            deps.api.addr_validate(&admin.clone().into_string())?;
            ADMIN.set(deps, Some(admin))?;
            Ok(Response::new())
        }
        SudoMsg::UpdateVotingPeriod { voting_period } => {
            let mut state = STATE.load(deps.storage)?;
            state.voting_period = voting_period;
            STATE.save(deps.storage, &state)?;
            Ok(Response::new())
        }
        SudoMsg::AddNewDelegation { delegation_info } => {
            let delegation = DELEGATION_INFO
                .may_load(deps.storage, delegation_info.delegated_address.clone())?;
            //// see all checks in the delegation_info
            if delegation.is_some() {
                return Err(ContractError::CustomError {
                    val: "Delegation already exists".to_string(),
                });
            }

            if delegation_info.protocol_fees > Decimal::one() {
                return Err(ContractError::CustomError {
                    val: "Protocol fees percentage cannot be greater than 100 %".to_string(),
                });
            }
            if delegation_info.protocol_fees < Decimal::zero() {
                return Err(ContractError::CustomError {
                    val: "Protocol fees percentage cannot be less than 0 %".to_string(),
                });
            }

            if delegation_info.delegated_address == delegation_info.fee_collector_adress {
                return Err(ContractError::CustomError {
                    val: "Delegator and fee collector address cannot be same".to_string(),
                });
            }

            DELEGATION_INFO.save(
                deps.storage,
                delegation_info.delegated_address.clone(),
                &delegation_info,
                env.block.height,
            )?;

            Ok(Response::new())
        }
        SudoMsg::UpdateExistingDelegation { delegation_info } => {
            let delegation = DELEGATION_INFO
                .may_load(deps.storage, delegation_info.delegated_address.clone())?;
            //// see all checks in the delegation_info
            if delegation.is_none() {
                return Err(ContractError::CustomError {
                    val: "Delegation does not exists".to_string(),
                });
            }

            if delegation_info.protocol_fees > Decimal::one() {
                return Err(ContractError::CustomError {
                    val: "Protocol fees percentage cannot be greater than 100 %".to_string(),
                });
            }
            if delegation_info.protocol_fees < Decimal::zero() {
                return Err(ContractError::CustomError {
                    val: "Protocol fees percentage cannot be less than 0 %".to_string(),
                });
            }

            if delegation_info.delegated_address == delegation_info.fee_collector_adress {
                return Err(ContractError::CustomError {
                    val: "Delegator and fee collector address cannot be same".to_string(),
                });
            }

            DELEGATION_INFO.save(
                deps.storage,
                delegation_info.delegated_address.clone(),
                &delegation_info,
                env.block.height,
            )?;

            Ok(Response::new())
        }
    }
}
