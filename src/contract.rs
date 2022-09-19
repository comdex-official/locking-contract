use crate::error::ContractError;
use crate::helpers::{
    get_token_supply, query_app_exists, query_extended_pair_by_app, query_get_asset_data,
    query_surplus_reward, query_whitelisted_asset,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SudoMsg};
use crate::state::{
    LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, STATE, SUPPLY,
    TOKENS, VTOKENS,
};
use crate::state::{
    Proposal, Vote, ADMIN, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, COMPLETEDPROPOSALS, EMISSION,
    MAXPROPOSALCLAIMED, PROPOSAL, PROPOSALCOUNT, PROPOSALVOTE, REBASE_CLAIMED, VOTERSPROPOSAL,
    VOTERS_VOTE,
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
    deps.api
        .addr_validate(&msg.vesting_contract.clone().into_string())?;
    deps.api.addr_validate(&msg.admin.clone().into_string())?;
    map_validate(deps.api, &msg.foundation_addr)?;
    //query_app_exists(deps.as_ref(), msg.emission.app_id)?;
    let mut foundation_addr_unique = msg.foundation_addr.clone();
    foundation_addr_unique.dedup();
    let state = State {
        t1: msg.t1,
        t2: msg.t2,
        t3: msg.t3,
        t4: msg.t4,
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
            val: "Pending rewards should be not be zero %".to_string(),
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
    //// Set Contract version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    //// Set State
    STATE.save(deps.storage, &state)?;
    EMISSION.save(deps.storage, msg.emission.app_id, &msg.emission)?;
    PROPOSALCOUNT.save(deps.storage, &0)?;
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
        } => {
            let app_response = query_app_exists(deps.as_ref(), app_id)?;

            //// get gov token denom for app
            let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;

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
            )
        }
        ExecuteMsg::RaiseProposal { app_id } => {
            //check if app exist
            query_app_exists(deps.as_ref(), app_id)?;
            ////get ext pairs vec from app
            let ext_pairs = query_extended_pair_by_app(deps.as_ref(), app_id)?;

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
        ExecuteMsg::ClaimReward { app_id } => claim_rewards(deps, env, info, app_id),
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
        ExecuteMsg::Transfer {
            recipient,
            locking_period,
            denom,
        } => handle_transfer(deps, env, info, recipient, locking_period, denom),
        ExecuteMsg::FoundationRewards { proposal_id } => {
            emission_foundation(deps, env, info, proposal_id)
        }
        ExecuteMsg::Rebase { proposal_id } => calculate_rebase_reward(deps, env, info, proposal_id),
    }
}

pub fn emission_foundation(
    deps: DepsMut<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
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

    Ok(Response::new()
        .add_messages(vec![emission_msg])
        .add_attribute("action", "Foundation_Emission")
        .add_attribute("from", info.sender))
}

fn lock_funds(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    _app_id: u64,
    sender: Addr,
    funds: Coin,
    locking_period: LockingPeriod,
) -> Result<(), ContractError> {
    let mut state = STATE.load(deps.storage)?;
    if state.min_lock_amount > funds.amount {
        return Err(ContractError::CustomError {
            val: "Lock amount less than minimum lock amount".to_string(),
        });
    }
    // Load the locking period and weight
    let PeriodWeight { period, weight } = get_period(state.clone(), locking_period.clone())?;

    let new_vtoken = create_vtoken(
        deps.storage,
        env.clone(),
        locking_period,
        period,
        weight,
        funds.clone(),
    )?;

    // Loads the NFT if present else create a new NFT
    let nft = TOKENS.may_load(deps.storage, sender.clone())?;

    match nft {
        Some(_) => {}
        None => {
            // Create a new NFT
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
            // If value exists for given key, then push new vtoken else update
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
    if let Some { .. } = recipient {
        deps.api
            .addr_validate(recipient.clone().unwrap().as_str())?;
        lock_funds(
            deps,
            env,
            app_id,
            recipient.unwrap(),
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

fn get_period(state: State, locking_period: LockingPeriod) -> Result<PeriodWeight, ContractError> {
    Ok(match locking_period {
        LockingPeriod::T1 => state.t1,
        LockingPeriod::T2 => state.t2,
        LockingPeriod::T3 => state.t3,
        LockingPeriod::T4 => state.t4,
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
) -> Result<Response<ComdexMessages>, ContractError> {
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    //Check active proposal

    let max_proposal_claimed = MAXPROPOSALCLAIMED
        .load(deps.storage, (app_id, info.sender.clone()))
        .unwrap_or_default();

    let all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };

    let mut bribe_coins = calculate_bribe_reward(
        deps.as_ref(),
        env.clone(),
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    )?;

    let surplus_share = calculate_surplus_reward(
        deps.as_ref(),
        env,
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    )?;

    if !bribe_coins.is_empty() {
        if !surplus_share.amount.is_zero() {
            for coin1 in bribe_coins.iter_mut() {
                if surplus_share.denom == coin1.denom {
                    coin1.amount += surplus_share.amount;
                }
            }
        }
    } else if !surplus_share.amount.is_zero() {
        bribe_coins = vec![surplus_share]
    } else {
        bribe_coins = vec![]
    }

    MAXPROPOSALCLAIMED.save(
        deps.storage,
        (app_id, info.sender.clone()),
        all_proposals.last().unwrap(),
    )?;

    bribe_coins.sort_by_key(|element| element.denom.clone());

    if !bribe_coins.is_empty() {
        Ok(Response::new()
            .add_attribute("method", "Bribe Claimed")
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
    max_proposal_claimed: u64,
    all_proposals: Vec<u64>,
    _app_id: u64,
) -> Result<Vec<Coin>, ContractError> {
    let mut bribe_coins: Vec<Coin> = vec![];
    for proposalid in all_proposals {
        if proposalid <= max_proposal_claimed {
            continue;
        }
        let vote = match VOTERSPROPOSAL.may_load(deps.storage, (info.sender.clone(), proposalid))? {
            Some(val) => val,
            None => continue,
        };

        let total_vote_weight = PROPOSALVOTE
            .load(deps.storage, (proposalid, vote.extended_pair))?
            .u128();
        let total_bribe =
            match BRIBES_BY_PROPOSAL.may_load(deps.storage, (proposalid, vote.extended_pair))? {
                Some(val) => val,
                None => vec![],
            };

        let mut claimable_bribe: Vec<Coin> = vec![];

        for coin in total_bribe.clone() {
            let claimable_amount = (Decimal::new(Uint128::from(vote.vote_weight))
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

    //// send bank message to band

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
    let mut locked_t3: u128 = 0;
    let mut locked_t4: u128 = 0;

    for vtoken in vtokens {
        match vtoken.period {
            LockingPeriod::T1 => locked_t1 += vtoken.token.amount.u128(),
            LockingPeriod::T2 => locked_t2 += vtoken.token.amount.u128(),
            LockingPeriod::T3 => locked_t3 += vtoken.token.amount.u128(),
            LockingPeriod::T4 => locked_t4 += vtoken.token.amount.u128(),
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
            denom: gov_token_denom.clone(),
        };
        lock_funds(
            deps.branch(),
            env.clone(),
            proposal.app_id,
            info.sender.clone(),
            fund_t2,
            LockingPeriod::T2,
        )?;
    }
    let lock_amount_t3 = Uint128::from(locked_t3).mul(Decimal::from_ratio(
        Uint128::from(total_rebase_amount),
        Uint128::from(total_locked),
    ));

    if lock_amount_t3 != Uint128::zero() {
        let fund_t3 = Coin {
            amount: lock_amount_t3,
            denom: gov_token_denom.clone(),
        };
        lock_funds(
            deps.branch(),
            env.clone(),
            proposal.app_id,
            info.sender.clone(),
            fund_t3,
            LockingPeriod::T3,
        )?;
    }
    let lock_amount_t4 = Uint128::from(locked_t4).mul(Decimal::from_ratio(
        Uint128::from(total_rebase_amount),
        Uint128::from(total_locked),
    ));

    if lock_amount_t4 != Uint128::zero() {
        let fund_t4 = Coin {
            amount: lock_amount_t4,
            denom: gov_token_denom,
        };
        lock_funds(
            deps.branch(),
            env,
            proposal.app_id,
            info.sender.clone(),
            fund_t4,
            LockingPeriod::T4,
        )?;
    }

    if lock_amount_t1 == Uint128::zero()
        && lock_amount_t2 == Uint128::zero()
        && lock_amount_t3 == Uint128::zero()
        && lock_amount_t4 == Uint128::zero()
    {
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
    let mut total_vote: Uint128 = Uint128::zero();
    for i in ext_pair {
        let vote = PROPOSALVOTE
            .load(deps.storage, (proposal_id, i))
            .unwrap_or_else(|_| Uint128::from(0_u32));
        votes.push(vote);
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

    let surplus = query_surplus_reward(deps.as_ref(), app_id, state.surplus_asset_id)?;
    proposal.total_surplus = surplus.clone();
    EMISSION.save(deps.storage, proposal.app_id, &emission)?;
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    let mut msg: Vec<ComdexMessages> = vec![];
    let app_id_param = app_id;

    let emission_msg = ComdexMessages::MsgEmissionRewards {
        app_id: app_id_param,
        amount: Uint128::from(proposal.emission_distributed),
        extended_pair: proposal.extended_pair,
        voting_ratio: votes,
    };

    let rebase_msg = ComdexMessages::MsgRebaseMint {
        app_id: app_id_param,
        amount: Uint128::from(proposal.rebase_distributed),
        contract_addr: env.contract.address.to_string(),
    };
    let surplus_msg = ComdexMessages::MsgGetSurplusFund {
        app_id: app_id_param,
        asset_id: state.surplus_asset_id,
        contract_addr: env.contract.address.into_string(),
        amount: surplus.clone(),
    };

    if total_vote != Uint128::zero() {
        msg.push(emission_msg);
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
    Ok(Response::new()
        .add_attribute("method", "emission")
        .add_messages(msg))
}

pub fn vote_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    proposal_id: u64,
    extended_pair: u64,
    gov_token_denom: String,
) -> Result<Response<ComdexMessages>, ContractError> {
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

    //// check if already voted for proposal
    let has_voted = VOTERS_VOTE
        .may_load(deps.storage, (info.sender.clone(), proposal_id))?
        .unwrap_or_default();

    // check if ext_pair param exist in extended pair list to vote for

    let extended_pairs = proposal.extended_pair.clone();

    //// check if extended pair exists in proposal's extended pair
    match extended_pairs.binary_search(&extended_pair) {
        Ok(_) => (),
        Err(_) => {
            return Err(ContractError::CustomError {
                val: "Invalid Extended pair".to_string(),
            })
        }
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

    // Update proposal Vote for an app

    let mut proposal_vote = PROPOSALVOTE
        .load(deps.storage, (proposal_id, extended_pair))
        .unwrap_or_default();

    // if already voted , update voting stats
    if has_voted {
        let prev_vote = VOTERSPROPOSAL.load(deps.storage, (info.sender.clone(), proposal_id))?;
        let last_vote_weight = prev_vote.vote_weight;
        let last_voted_pair = prev_vote.extended_pair;
        if last_voted_pair == extended_pair {
            proposal_vote -= Uint128::from(last_vote_weight);
            proposal_vote += Uint128::from(vote_power);
            proposal.total_voted_weight -= last_vote_weight;
            proposal.total_voted_weight += vote_power;
            PROPOSALVOTE.save(deps.storage, (proposal_id, extended_pair), &proposal_vote)?;
            PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
            let app_id_param = app_id;
            let extended_pair_param = extended_pair;
            let vote = Vote {
                app_id: app_id_param,
                extended_pair: extended_pair_param,
                vote_weight: vote_power,
            };

            VOTERSPROPOSAL.save(deps.storage, (info.sender.clone(), proposal_id), &vote)?;
        } else {
            let mut prev_proposal_vote = PROPOSALVOTE
                .load(deps.storage, (proposal_id, last_voted_pair))
                .unwrap_or_default();

            prev_proposal_vote -= Uint128::from(last_vote_weight);
            proposal_vote += Uint128::from(vote_power);
            PROPOSALVOTE.save(deps.storage, (proposal_id, extended_pair), &proposal_vote)?;
            PROPOSALVOTE.save(
                deps.storage,
                (proposal_id, last_voted_pair),
                &prev_proposal_vote,
            )?;

            proposal.total_voted_weight -= last_vote_weight;
            proposal.total_voted_weight += vote_power;
            PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
            let app_id_param = app_id;
            let extended_pair_param = extended_pair;

            let vote = Vote {
                app_id: app_id_param,
                extended_pair: extended_pair_param,
                vote_weight: vote_power,
            };

            VOTERSPROPOSAL.save(deps.storage, (info.sender.clone(), proposal_id), &vote)?;
        }
    } else {
        let updated_vote = Uint128::from(vote_power) + proposal_vote;
        PROPOSALVOTE.save(deps.storage, (proposal_id, extended_pair), &updated_vote)?;

        let app_id_param = app_id;
        let extended_pair_param = extended_pair;
        let vote = Vote {
            app_id: app_id_param,
            extended_pair: extended_pair_param,
            vote_weight: vote_power,
        };
        proposal.total_voted_weight += vote_power;
        VOTERSPROPOSAL.save(deps.storage, (info.sender.clone(), proposal_id), &vote)?;
        PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    }

    VOTERS_VOTE.save(deps.storage, (info.sender, proposal_id), &true)?;

    // update proposal

    Ok(Response::new()
        .add_attribute("method", "voted for proposal")
        .add_attribute("voted on", extended_pair.to_string()))
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

    // set the new version
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    // do any desired state migrations...

    let voting_period=msg.voting_period;
    let mut state = STATE.load(deps.storage)?;
    state.voting_period = voting_period;
    STATE.save(deps.storage, &state)?;
    ADMIN.set(deps, Some(msg.admin_address))?;

    Ok(Response::default())
}

#[entry_point]
pub fn sudo(deps: DepsMut, _env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
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
            emission.emission_rate = emission_rate;
            EMISSION.save(deps.storage, emission.app_id, &emission)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateFoundationInfo {
            addresses,
            foundation_percentage,
        } => {
            let mut state = STATE.load(deps.storage)?;
            map_validate(deps.api, &addresses)?;
            state.foundation_addr = addresses;
            state.foundation_percentage = foundation_percentage;
            STATE.save(deps.storage, &state)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateLockingPeriod { t1, t2, t3, t4 } => {
            let mut state = STATE.load(deps.storage)?;
            state.t1 = t1;
            state.t2 = t2;
            state.t3 = t3;
            state.t4 = t4;
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
    }
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use crate::state::Emission;
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{coin, coins, Addr, CosmosMsg, OwnedDeps, StdError};

    const DENOM: &str = "TKN";
    /// Returns default InstantiateMsg with each value in seconds.
    /// - t1 is 1 week (7*24*60*60), similarly, t2 is 2 weeks, t3 is 3 weeks
    /// and t4 is 4 weeks.
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
            voting_period: 30000,
            vesting_contract: Addr::unchecked("vesting_contract"),
            foundation_addr: vec!["fd1".to_ascii_lowercase(), "fd2".to_ascii_lowercase()],
            foundation_percentage: Decimal::percent(2),
            surplus_asset_id: 3,
            emission: Emission {
                app_id: 1,
                total_rewards: 200000,
                rewards_pending: 200000,
                emission_rate: Decimal::percent(2),
                distributed_rewards: 0,
            },
            admin: Addr::unchecked("admin"),
            min_lock_amount: Uint128::from(1 as u128),
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

        // Successful execution
        let info = mock_info("user1", &coins(100, DENOM.to_string()));

        //let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
        let res =
            handle_lock_nft(deps.as_mut(), env.clone(), info, 1, LockingPeriod::T1, None).unwrap();

        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes.len(), 2);

        let sender_addr = Addr::unchecked("user1");
        let token = TOKENS.load(&deps.storage, sender_addr.clone()).unwrap();

        assert_eq!(token.owner, sender_addr.clone());
        assert_eq!(token.token_id, 1u64);
        // .token should be the same as locked tokens

        // Check to see the SUPPLY mapping is correct
        let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
        assert_eq!(supply.token, 100u128);
        assert_eq!(supply.vtoken, 25u128);
    }

    #[test]
    fn lock_different_denom_and_time_period() {
        // mock values
        let mut deps = mock_dependencies();
        let env = mock_env();
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

        let nft = VTOKENS
            .load(deps.as_ref().storage, (owner_addr.clone(), "DNM1"))
            .unwrap();
        let nft2 = VTOKENS
            .load(deps.as_ref().storage, (owner_addr.clone(), "DNM2"))
            .unwrap();

        assert_eq!(nft.len(), 1);
        assert_eq!(nft[0].token.denom, "DNM1".to_string());
        assert_eq!(nft2[0].token.denom, "DNM2".to_string());

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
        let old_start_time = env.block.time;
        env.block.time = env.block.time.plus_seconds(100_000);

        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // Check correct updating in nft
        let nft = VTOKENS
            .load(deps.as_ref().storage, (owner_addr.clone(), "TKN"))
            .unwrap();

        assert_eq!(nft.len(), 2);
        assert_eq!(nft[0].token.amount.u128(), 100u128);
        assert_eq!(nft[0].vtoken.amount.u128(), 25u128);
        assert_eq!(nft[0].start_time, old_start_time);
        assert_eq!(nft[0].end_time, old_start_time.plus_seconds(imsg.t1.period));
        assert_eq!(nft[0].period, LockingPeriod::T1);
        assert_eq!(nft[0].status, Status::Locked);

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

        // Check correct updating in nft
        let nft = VTOKENS
            .load(deps.as_ref().storage, (owner_addr.clone(), "TKN"))
            .unwrap();

        assert_eq!(nft.len(), 2);
        assert_eq!(nft[0].token.amount.u128(), 100u128);
        assert_eq!(nft[0].vtoken.amount.u128(), 25u128);
        assert_eq!(nft[1].token.amount.u128(), 100u128);
        assert_eq!(nft[1].vtoken.amount.u128(), 50u128);
        assert_eq!(nft[1].start_time, env.block.time);
        assert_eq!(nft[1].end_time, env.block.time.plus_seconds(imsg.t2.period));
        assert_eq!(nft[0].period, LockingPeriod::T1);
        assert_eq!(nft[0].status, Status::Locked);
        assert_eq!(nft[1].period, LockingPeriod::T2);
        assert_eq!(nft[1].status, Status::Locked);

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

        let _owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));

        let _res = handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        env.block.time = env.block.time.plus_seconds(imsg.t1.period + 1u64);

        // Withdrawing 10 Tokens
        let info = mock_info("owner", &[]);
        let res =
            handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string()).unwrap();
        assert_eq!(res.messages.len(), 1);
        assert_eq!(res.attributes.len(), 2);
        assert_eq!(
            res.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![coin(100, DENOM.to_string())]
            })
        );

        // Just a check if the order matters with Decimal
        let vtoken_balance1 = Uint128::from(90u64) * imsg.t1.weight;
        let vtoken_balance2 = imsg.t1.weight * Uint128::from(90u64);
        assert_eq!(vtoken_balance1, vtoken_balance2);
    }

    #[test]
    fn withdraw_no_vtokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        let res = handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string())
            .unwrap_err();
        match res {
            ContractError::NotFound { .. } => {}
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

        let res = handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap_err();
        match res {
            ContractError::InsufficientFunds { .. } => {}
            e => panic!("{:?}", e),
        };
        let _owner = Addr::unchecked("owner");
        let _info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string())
            .unwrap_err();
        match res {
            ContractError::NotFound { .. } => {}
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

        let _owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let _res = handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("owner", &[]);
        let res = handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string())
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

        let _owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let _res = handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("owner", &[]);
        let res = handle_withdraw(deps.as_mut(), env.clone(), info.clone(), "DNM1".to_string())
            .unwrap_err();
        match res {
            ContractError::NotFound { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn transfer_different_denom() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        let owner = Addr::unchecked("owner");
        let recipient = Addr::unchecked("recipient");

        let denom1 = "DNM1";
        let denom2 = "DNM2";

        // Create token for recipient
        let info = mock_info("recipient", &coins(100, denom2.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info,
            12,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // Create tokens for owner == sender
        let info = mock_info("owner", &coins(100, denom1.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            12,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // create a copy of owner's vtoken to compare and check if the recipient's
        // vtoken is the same.
        let locked_vtokens = VTOKENS
            .load(deps.as_ref().storage, (owner.clone(), denom1))
            .unwrap();

        let msg = ExecuteMsg::Transfer {
            recipient: recipient.to_string(),
            locking_period: LockingPeriod::T1,
            denom: denom1.to_string(),
        };

        let info = mock_info(owner.as_str(), &[]);
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes.len(), 3);

        // Check correct update in sender vtokens
        let res = VTOKENS
            .load(deps.as_ref().storage, (owner.clone(), denom1))
            .unwrap_err();
        match res {
            StdError::NotFound { .. } => {}
            e => panic!("{:?}", e),
        }

        // Check correct update in recipient vtokens
        {
            let res = VTOKENS
                .load(deps.as_ref().storage, (recipient.clone(), denom1))
                .unwrap();
            assert_eq!(res.len(), 1);
            assert_eq!(res[0], locked_vtokens[0]);

            let res = VTOKENS
                .load(deps.as_ref().storage, (recipient.clone(), denom2))
                .unwrap();
            assert_eq!(res.len(), 1);
            assert_eq!(res[0].token.amount.u128(), 100);
            assert_eq!(res[0].token.denom, denom2.to_string());
        }

        // Check correct update in recipient nft
        let recipient_nft = VTOKENS
            .load(deps.as_ref().storage, (recipient.clone(), "DNM1"))
            .unwrap();

        assert_eq!(recipient_nft.len(), 1);
        assert_eq!(recipient_nft[0].token.amount.u128(), 100);
        assert_eq!(recipient_nft[0].token.denom, denom1.to_string());
    }

    #[test]
    fn transfer_same_denom() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("admin", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

        let sender = Addr::unchecked("sender");
        let recipient = Addr::unchecked("recipient");

        // Lock tokens for sender
        let info = mock_info(sender.as_str(), &coins(1000, DENOM.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info,
            12,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // Lock tokens for recipient
        let info = mock_info(recipient.as_str(), &coins(1000, DENOM.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info,
            12,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        // Transfer tokens to recipient
        let info = mock_info(sender.as_str(), &[]);
        handle_transfer(
            deps.as_mut(),
            env.clone(),
            info,
            recipient.to_string(),
            LockingPeriod::T1,
            DENOM.to_string(),
        )
        .unwrap();

        // Check VTOKENS
        VTOKENS
            .load(deps.as_ref().storage, (sender.clone(), DENOM))
            .unwrap_err();

        let recipient_vtokens = VTOKENS
            .load(deps.as_ref().storage, (recipient.clone(), DENOM))
            .unwrap();
        assert_eq!(recipient_vtokens.len(), 2);
        assert_eq!(recipient_vtokens[0].token.amount.u128(), 1000);
        assert_eq!(recipient_vtokens[0].token.denom, DENOM.to_string());
        assert_eq!(recipient_vtokens[0].vtoken.amount.u128(), 250);
        assert_eq!(recipient_vtokens[0].vtoken.denom, "vTKN".to_string());
        assert_eq!(recipient_vtokens[0].start_time, env.block.time);
        assert_eq!(
            recipient_vtokens[0].end_time,
            env.block.time.plus_seconds(imsg.t1.period)
        );
        assert_eq!(recipient_vtokens[1].token.amount.u128(), 1000);
        assert_eq!(recipient_vtokens[1].vtoken.amount.u128(), 250);
    }

    #[test]
    fn raise_init() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();
    }

    #[test]
    fn test_non_admin_proposal_error() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
        let res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap_err();
        match res {
            ContractError::Admin { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn proposal_successfully_set() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("admin", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
        let _res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
        let current_proposal = APPCURRENTPROPOSAL.load(deps.as_ref().storage, 1).unwrap();
        assert_eq!(current_proposal, 1);
    }

    #[test]
    fn no_consecutive_proposals() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("admin", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
        let _res =
            raise_proposal(deps.as_mut(), env.clone(), info.clone(), 1, vec![1, 2, 3]).unwrap();
        let res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap_err();
        match res {
            ContractError::CustomError { .. } => {}
            e => panic!("{:?}", e),
        };
        let current_proposal = APPCURRENTPROPOSAL.load(deps.as_ref().storage, 1).unwrap();
        assert_eq!(current_proposal, 1);
    }

    #[test]
    fn no_pair_for_proposals() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("admin", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
        let res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![]).unwrap_err();
        match res {
            ContractError::CustomError { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn test_vote_proposal_without_locking() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        let _res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

        let info = mock_info("voter1", &[]);

        let err = vote_proposal(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            1,
            1,
            DENOM.to_string(),
        );
        assert_eq!(
            err,
            Err(ContractError::CustomError {
                val: "No tokens locked to perform voting on proposals".to_string()
            })
        );
    }

    #[test]

    fn test_vote_proposal_with_wrong_extended_pair() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

        let info = mock_info("voter1", &[]);

        let err = vote_proposal(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            1,
            6,
            DENOM.to_string(),
        );
        assert_eq!(
            err,
            Err(ContractError::CustomError {
                val: "Invalid Extended pair".to_string()
            })
        );
    }

    #[test]

    fn test_bribe_proposal() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

        let info = mock_info(
            "voter1",
            &[Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            }],
        );

        bribe_proposal(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            1,
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
        )
        .unwrap();
    }

    #[test]

    fn test_bribe_proposal_wrong_pair() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

        let info = mock_info(
            "voter1",
            &[Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            }],
        );

        let err = bribe_proposal(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            6,
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
        );
        assert_eq!(
            err,
            Err(ContractError::CustomError {
                val: "Invalid Extended pair".to_string()
            })
        );
    }

    #[test]

    fn test_bribe_proposal_multiple_denoms() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

        let info = mock_info(
            "voter1",
            &[
                Coin {
                    amount: Uint128::from(200_u128),
                    denom: "bribe1".to_string(),
                },
                Coin {
                    amount: Uint128::from(200_u128),
                    denom: "bribe2".to_string(),
                },
            ],
        );

        let err = bribe_proposal(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            1,
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
        );
        assert_eq!(
            err,
            Err(ContractError::CustomError {
                val: "Multiple denominations are not supported as yet.".to_string()
            })
        );
    }

    #[test]

    fn test_foundation_emission_no_completed() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

        let info = mock_info(
            "admin",
            &[
                Coin {
                    amount: Uint128::from(200_u128),
                    denom: "bribe1".to_string(),
                },
                Coin {
                    amount: Uint128::from(200_u128),
                    denom: "bribe2".to_string(),
                },
            ],
        );

        let err = emission_foundation(deps.as_mut(), env.clone(), info.clone(), 1);
        assert_eq!(
            err,
            Err(ContractError::CustomError {
                val: "Emission calculation did not take place to initiate foundation calculation"
                    .to_string()
            })
        );
    }

    #[test]

    fn test_foundation_emission_accept_no_fund() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
        let mut proposal = PROPOSAL.load(deps.as_ref().storage, 1).unwrap();
        proposal.emission_completed = true;
        proposal.foundation_distributed = 10000000000;
        _ = PROPOSAL.save(deps.as_mut().storage, 1, &proposal);
        let info = mock_info(
            "admin",
            &[
                Coin {
                    amount: Uint128::from(200_u128),
                    denom: "bribe1".to_string(),
                },
                Coin {
                    amount: Uint128::from(200_u128),
                    denom: "bribe2".to_string(),
                },
            ],
        );

        let err = emission_foundation(deps.as_mut(), env.clone(), info.clone(), 1);
        assert_eq!(
            err,
            Err(ContractError::CustomError {
                val: "Funds not accepted".to_string()
            })
        );
    }

    #[test]

    fn test_foundation_emission() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
        let mut proposal = PROPOSAL.load(deps.as_ref().storage, 1).unwrap();
        proposal.emission_completed = true;
        proposal.foundation_distributed = 10000000000;
        _ = PROPOSAL.save(deps.as_mut().storage, 1, &proposal);
        let info = mock_info("admin", &[]);

        let _response = emission_foundation(deps.as_mut(), env.clone(), info.clone(), 1);
    }

    #[test]

    fn test_bribe_reward() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();

        let info = mock_info("voter1", &coins(100, DENOM.to_string()));
        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            LockingPeriod::T1,
            None,
        )
        .unwrap();

        let info = mock_info("admin", &[]);
        raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
        let mut proposal = PROPOSAL.load(deps.as_ref().storage, 1).unwrap();
        proposal.emission_completed = true;
        proposal.foundation_distributed = 10000000000;
        _ = PROPOSAL.save(deps.as_mut().storage, 1, &proposal);
        let info = mock_info(
            "voter1",
            &[Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            }],
        );

        bribe_proposal(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            1,
            1,
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
        )
        .unwrap();

        let vote = Vote {
            app_id: 1,
            extended_pair: 1,
            vote_weight: 200,
        };
        _ = VOTERSPROPOSAL.save(deps.as_mut().storage, (Addr::unchecked("voter1"), 1), &vote);
        _ = PROPOSALVOTE.save(deps.as_mut().storage, (1, 1), &Uint128::from(500_u128));

        let info = mock_info("voter1", &[]);

        let response =
            calculate_bribe_reward(deps.as_ref(), env.clone(), info.clone(), 0, vec![1], 1)
                .unwrap();
        assert_eq!(
            response,
            vec![Coin {
                denom: "bribe1".to_ascii_lowercase(),
                amount: Uint128::from(80_u128)
            }]
        );
    }

    #[test]

    fn test_rebase_formula() {
        let total_locked: u128 = 10000_u128;

        let my_locked: u128 = 222_u128;

        let rebase_amount: u128 = 20000_u128;

        let lock_amount_t1 = Uint128::from(my_locked).mul(Decimal::from_ratio(
            Uint128::from(rebase_amount),
            Uint128::from(total_locked),
        ));

        assert_eq!(lock_amount_t1, Uint128::new(444_u128));
    }

    #[test]

    fn test_bribe_formula() {
        let vote_weight: u128 = 30_u128;

        let total_vote: u128 = 100_u128;

        let claimable_amount = (Decimal::new(Uint128::from(vote_weight))
            .div(Decimal::new(Uint128::from(total_vote))))
        .mul(Uint128::from(500_u128));

        assert_eq!(claimable_amount, Uint128::new(150_u128));
    }
}
