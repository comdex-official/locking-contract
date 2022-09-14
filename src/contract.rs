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
    LOCKINGADDRESS, MAXPROPOSALCLAIMED, PROPOSAL, PROPOSALCOUNT, PROPOSALVOTE, VOTERSPROPOSAL,
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
    query_app_exists(deps.as_ref(), msg.emission.app_id)?;
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
    };

    if msg.foundation_percentage > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Foundation Emission percentage cannot be greater than 100 %".to_string(),
        });
    }
    if msg.emission.rewards_pending != 0 {
        return Err(ContractError::CustomError {
            val: "Pending rewards should be zero %".to_string(),
        });
    }
    if msg.emission.distributed_rewards != 0 {
        return Err(ContractError::CustomError {
            val: "Distributed rewards should be zero".to_string(),
        });
    }

    if msg.emission.emmission_rate > Decimal::one() {
        return Err(ContractError::CustomError {
            val: "Emission rate cannot be greater one".to_string(),
        });
    }
    //// Set Contract version
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    //// Set State
    STATE.save(deps.storage, &state)?;

    EMISSION.save(deps.storage, msg.emission.app_id, &msg.emission)?;

    ADMIN.save(deps.storage, &msg.admin)?;

    PROPOSALCOUNT.save(deps.storage, &0)?;

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
        } => vote_proposal(deps, env, info, app_id, proposal_id, extended_pair),
        ExecuteMsg::RaiseProposal { app_id } => raise_proposal(deps, env, info, app_id),
        ExecuteMsg::Bribe {
            proposal_id,
            extended_pair,
        } => bribe_proposal(deps, env, info, proposal_id, extended_pair),
        ExecuteMsg::ClaimReward { app_id } => claim_rewards(deps, env, info, app_id),
        ExecuteMsg::Emmission { proposal_id } => emission(deps, env, info, proposal_id),
        ExecuteMsg::Lock {
            app_id,
            locking_period,
        } => handle_lock_nft(deps, env, info, app_id, locking_period),
        ExecuteMsg::Withdraw { denom } => handle_withdraw(deps, env, info, denom),
        ExecuteMsg::Transfer {
            recipent,
            locking_period,
            denom,
        } => handle_transfer(deps, env, info, recipent, locking_period, denom),
        ExecuteMsg::FoundationRewards { proposal_id } => {
            emission_foundation(deps, env, info, proposal_id)
        }
        ExecuteMsg::Rebase {
            proposal_id,
            app_id,
        } => calculate_rebase_reward(deps, env, info, proposal_id, app_id),
    }
}

pub fn emission_foundation(
    deps: DepsMut<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::CustomError {
            val: "Unauthorized".to_string(),
        });
    }
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    // check emission already compluted and executed
    if !proposal.emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission calculation did not take place to initiate foundation calculation"
                .to_string(),
        });
    }

    //check if foundation emission has not taken plase for the proposal
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

    let mut addresses = match LOCKINGADDRESS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };

    match addresses.contains(&info.sender) {
        true => (),
        false => {
            addresses.push(info.sender.clone());
        }
    }

    LOCKINGADDRESS.save(deps.storage, app_id, &addresses)?;

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

    lock_funds(
        deps,
        env,
        app_id,
        info.sender.clone(),
        info.funds[0].clone(),
        locking_period,
    )?;

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

/// Update the SUPPLY map for the toal supply for vtokens and the corresponding
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

    // Retrive unlocked tokens with the given locking period
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

    // Load the sender denom that needs to be transfered
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

    // CHECK IF BRIBE ASSET EXISTS ON-CHAIN
    let bribe_coin = info.funds[0].clone();
    let found = query_whitelisted_asset(deps.as_ref(), bribe_coin.denom.clone())?;
    if !found {
        return Err(ContractError::CustomError {
            val: String::from("Asset not whitelisted on chain"),
        });
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

    let mut back_messages: Vec<BankMsg> = vec![];
    let surplus_share = calculate_surplus_reward(
        deps.as_ref(),
        env,
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    )?;
    bribe_coins.sort_by_key(|element| element.denom.clone());
    if !bribe_coins.is_empty() {
        let bribe = BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: bribe_coins.clone(),
        };
        back_messages.push(bribe);
    }

    if !surplus_share.amount.is_zero() {
        for coin1 in bribe_coins.iter_mut() {
            if surplus_share.denom == coin1.denom {
                coin1.amount += surplus_share.amount;
            }
        }
    }

    MAXPROPOSALCLAIMED.save(
        deps.storage,
        (app_id, info.sender.clone()),
        all_proposals.last().unwrap(),
    )?;

    if !back_messages.is_empty() {
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
                amount: Uint128::from(claimable_amount),
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
    app_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    //// only admin can execute
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::CustomError {
            val: "Unauthorized".to_string(),
        });
    }
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;

    if proposal.rebase_completed {
        return Err(ContractError::CustomError {
            val: String::from("Rebase already completed"),
        });
    }
    let total_rebase_amount: u128 = proposal.rebase_distributed;

    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;

    let vtokenholders = LOCKINGADDRESS.load(deps.storage, app_id)?;
    if vtokenholders.is_empty() {
        return Err(ContractError::CustomError {
            val: "No locked users to rebase".to_string(),
        });
    }

    for addr in vtokenholders.iter() {
        //// get v-tokens at proposal height
        let vtokens = match VTOKENS.may_load_at_height(
            deps.storage,
            (addr.to_owned(), &gov_token_denom),
            proposal.height,
        )? {
            Some(val) => val,
            None => vec![],
        };
        if vtokens.is_empty() {
            continue;
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

        for vtoken in vtokens.clone() {
            match vtoken.period {
                LockingPeriod::T1 => locked_t1 += vtoken.token.amount.u128(),
                LockingPeriod::T2 => locked_t2 += vtoken.token.amount.u128(),
                LockingPeriod::T3 => locked_t3 += vtoken.token.amount.u128(),
                LockingPeriod::T4 => locked_t4 += vtoken.token.amount.u128(),
            }
        }

        let total_share = locked_t1 + locked_t2 + locked_t3 + locked_t4;

        //// lock in t1
        let lock_amount_t1 = (Decimal::new(Uint128::from(locked_t1))
            .div(Decimal::new(Uint128::from(total_share))))
        .mul(
            (Decimal::new(Uint128::from(total_rebase_amount))
                .div(Decimal::new(Uint128::from(total_locked)))),
        )
        .mul(Uint128::from(1_u128));

        if lock_amount_t1 != Uint128::zero() {
            let fund_t1 = Coin {
                amount: Uint128::from(lock_amount_t1),
                denom: gov_token_denom.clone(),
            };

            lock_funds(
                deps.branch(),
                env.clone(),
                app_id,
                addr.clone(),
                fund_t1,
                LockingPeriod::T1,
            )?;
        }
        let lock_amount_t2 = (Decimal::new(Uint128::from(locked_t2))
            .div(Decimal::new(Uint128::from(total_share))))
        .mul(
            (Decimal::new(Uint128::from(total_rebase_amount))
                .div(Decimal::new(Uint128::from(total_locked)))),
        )
        .mul(Uint128::from(1_u128));

        if lock_amount_t2 != Uint128::zero() {
            let fund_t2 = Coin {
                amount: Uint128::from(lock_amount_t2),
                denom: gov_token_denom.clone(),
            };
            lock_funds(
                deps.branch(),
                env.clone(),
                app_id,
                addr.clone(),
                fund_t2,
                LockingPeriod::T2,
            )?;
        }
        let lock_amount_t3 = (Decimal::new(Uint128::from(locked_t3))
            .div(Decimal::new(Uint128::from(total_share))))
        .mul(
            (Decimal::new(Uint128::from(total_rebase_amount))
                .div(Decimal::new(Uint128::from(total_locked)))),
        )
        .mul(Uint128::from(1_u128));

        if lock_amount_t3 != Uint128::zero() {
            let fund_t3 = Coin {
                amount: Uint128::from(lock_amount_t3),
                denom: gov_token_denom.clone(),
            };
            lock_funds(
                deps.branch(),
                env.clone(),
                app_id,
                addr.clone(),
                fund_t3,
                LockingPeriod::T3,
            )?;
        }
        let lock_amount_t4 = (Decimal::new(Uint128::from(locked_t4))
            .div(Decimal::new(Uint128::from(total_share))))
        .mul(
            (Decimal::new(Uint128::from(total_rebase_amount))
                .div(Decimal::new(Uint128::from(total_locked)))),
        )
        .mul(Uint128::from(1_u128));

        if lock_amount_t4 != Uint128::zero() {
            let fund_t4 = Coin {
                amount: Uint128::from(lock_amount_t4),
                denom: gov_token_denom.clone(),
            };
            lock_funds(
                deps.branch(),
                env.clone(),
                app_id,
                addr.clone(),
                fund_t4,
                LockingPeriod::T4,
            )?;
        }
    }

    proposal.rebase_completed = true;
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;

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
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::CustomError {
            val: "Unauthorized".to_string(),
        });
    }
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
    let reward_emision = Uint128::from(emission.rewards_pending) * emission.emmission_rate;
    let effective_emission = reward_emision.mul(Decimal::one() - percentage_locked);
    // mint and distribue to vault owner  based vote portion
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
    proposal.rebase_distributed = (reward_emision.mul(percentage_locked)).u128();
    //// EMISSION Data Update
    emission.rewards_pending -= reward_emision.u128();
    emission.distributed_rewards += reward_emision.u128();

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
) -> Result<Response<ComdexMessages>, ContractError> {
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

    //// get App Data
    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    //// get gov token denom for app
    let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;

    if gov_token_denom.is_empty() || app_response.gov_token_id == 0 {
        return Err(ContractError::CustomError {
            val: "Gov token not found for the app".to_string(),
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
) -> Result<Response<ComdexMessages>, ContractError> {
    //// only admin can execute
    let admin = ADMIN.load(deps.storage)?;
    if info.sender != admin {
        return Err(ContractError::CustomError {
            val: "Unauthorized".to_string(),
        });
    }

    // do not accept funds
    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    //check if app exist
    query_app_exists(deps.as_ref(), app_id)?;
    ////get ext pairs vec from app
    let ext_pairs = query_extended_pair_by_app(deps.as_ref(), app_id)?;

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
        }, // unintialized dummy token
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
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
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

    Ok(Response::default())
}

#[entry_point]
pub fn sudo(deps: DepsMut, _env: Env, msg: SudoMsg) -> Result<Response, ContractError> {
    match msg {
        SudoMsg::UpdateVestingContract { address } => {
            let mut state = STATE.load(deps.storage)?;
            state.vesting_contract = deps.api.addr_validate(&address.clone().into_string())?;
            STATE.save(deps.storage, &state)?;
            Ok(Response::new())
        }
        SudoMsg::UpdateEmissionRate {
            emission_rate,
            app_id,
        } => {
            let mut emission = EMISSION.load(deps.storage, app_id)?;
            emission.emmission_rate = emission_rate;
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
            ADMIN.save(deps.storage, &admin)?;
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
    use cosmwasm_std::{coin, coins, Addr, CosmosMsg, OwnedDeps, StdError, Timestamp};

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
            voting_period: 604_800,
            vesting_contract: Addr::unchecked("vesting_contract"),
            foundation_addr: vec![],
            foundation_percentage: Decimal::new(Uint128::from(2 as u32)),
            surplus_asset_id: 3,
            emission: Emission {
                app_id: 1,
                total_rewards: 200000,
                rewards_pending: 200000,
                emmission_rate: Decimal::new(Uint128::from(2 as u64)),
                distributed_rewards: 123333,
            },
            admin: Addr::unchecked("admin"),
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
        )
        .unwrap();

        let info = mock_info("owner", &coins(100, "DNM2".to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            10,
            LockingPeriod::T2,
        )
        .unwrap();

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
        )
        .unwrap();

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
        )
        .unwrap();

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
        };

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));

        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

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

        // Check correct update in VTOKENS
        let _vtoken = VTOKENS
            .load(&deps.storage, (info.sender.clone(), DENOM))
            .unwrap_err();
        // assert_eq!(vtoken.len(), 1);
        // assert_eq!(vtoken[0].token.amount.u128(), 90u128);
        // let vtoken_balance = Uint128::from(25u64).sub(Uint128::from(10u64) * imsg.t1.weight);
        // assert_eq!(vtoken[0].vtoken.amount.u128(), vtoken_balance.u128());
        // assert_eq!(vtoken[0].status, Status::Unlocked);
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

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
        };
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap_err();
        match _res {
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

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
        };

        let _owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

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

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
        };

        let _owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let _res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let info = mock_info("owner", &[]);
        let res = handle_withdraw(deps.as_mut(), env.clone(), info.clone(), "DNM1".to_string())
            .unwrap_err();
        match res {
            ContractError::NotFound { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn transfer_to_new_user() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &[]);

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // Lock tokens
        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
        };

        let owner = Addr::unchecked("owner");
        let recipient = Addr::unchecked("recipient");

        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let locked_vtokens = VTOKENS
            .load(deps.as_ref().storage, (owner.clone(), DENOM))
            .unwrap();

        let msg = ExecuteMsg::Transfer {
            recipent: recipient.to_string(),
            locking_period: LockingPeriod::T1,
            denom: DENOM.to_string(),
        };

        let info = mock_info(owner.as_str(), &[]);
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        assert_eq!(res.messages.len(), 0);
        assert_eq!(res.attributes.len(), 3);

        // Check correct update in sender vtokens
        let res = VTOKENS
            .load(deps.as_ref().storage, (owner.clone(), DENOM))
            .unwrap_err();
        match res {
            StdError::NotFound { .. } => {}
            e => panic!("{:?}", e),
        }

        // Check correct update in recipient vtokens
        let res = VTOKENS
            .load(deps.as_ref().storage, (recipient.clone(), DENOM))
            .unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], locked_vtokens[0]);

        // Check correct update in recipient nft
        let recipient_nft = TOKENS
            .load(deps.as_ref().storage, recipient.clone())
            .unwrap();
        assert_eq!(recipient_nft.owner, recipient.clone());
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
        handle_lock_nft(deps.as_mut(), env.clone(), info, 12, LockingPeriod::T1).unwrap();

        // Create tokens for owner == sender
        let info = mock_info("owner", &coins(100, denom1.to_string()));
        handle_lock_nft(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            12,
            LockingPeriod::T1,
        )
        .unwrap();

        // create a copy of owner's vtoken to compare and check if the recipient's
        // vtoken is the same.
        let locked_vtokens = VTOKENS
            .load(deps.as_ref().storage, (owner.clone(), denom1))
            .unwrap();

        let msg = ExecuteMsg::Transfer {
            recipent: recipient.to_string(),
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

        // Check correct update in sender nft
        let sender_nft = TOKENS.load(deps.as_ref().storage, owner.clone()).unwrap();

        // Check correct update in recipient nft
        let recipient_nft = TOKENS
            .load(deps.as_ref().storage, recipient.clone())
            .unwrap();
        assert_eq!(recipient_nft.owner, recipient.clone());
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
        handle_lock_nft(deps.as_mut(), env.clone(), info, 12, LockingPeriod::T1).unwrap();

        // Lock tokens for recipient
        let info = mock_info(recipient.as_str(), &coins(1000, DENOM.to_string()));
        handle_lock_nft(deps.as_mut(), env.clone(), info, 12, LockingPeriod::T1).unwrap();

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
    fn raise_proposal_non_admin() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

        // Raise proposal request from non admin
        let info = mock_info("not_admin", &[]);
        let msg = ExecuteMsg::RaiseProposal { app_id: 1 };

        execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    }

    #[test]
    fn raise_proposal_funds_sent() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

        // Raise proposal request from non admin
        let info = mock_info("admin", &coins(100, DENOM.to_string()));
        let msg = ExecuteMsg::RaiseProposal { app_id: 1 };

        execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    }

    #[test]
    fn raise_proposal_basic_functionality() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

        // Raise proposal request from admin
        let info = mock_info("admin", &[]);
        let msg = ExecuteMsg::RaiseProposal { app_id: 1 };

        // Sucessful execution
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    }

    #[test]
    fn vote_proposal_invalid_proposal_id() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

        // Invalid proposal id
        let info = mock_info("voter", &[]);
        let msg = ExecuteMsg::VoteProposal {
            app_id: 1,
            proposal_id: 23,
            extended_pair: 3,
        };

        execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
    }

    #[test]
    fn vote_proposal_not_in_voting_period() {
        // Mock dependencies
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(1);
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

        // Create a proposal
        let proposal = Proposal {
            app_id: 1,
            voting_start_time: Timestamp::from_seconds(7),
            voting_end_time: Timestamp::from_seconds(10),
            extended_pair: vec![],
            emission_completed: false,
            rebase_completed: false,
            foundation_emission_completed: false,
            emission_distributed: 0,
            rebase_distributed: 0,
            foundation_distributed: 0,
            total_surplus: coin(0, DENOM),
            total_voted_weight: 12,
            height: 50,
        };
        PROPOSAL.save(deps.as_mut().storage, 23, &proposal).unwrap();

        // Set current block time past the voting_end_time
        env.block.time = Timestamp::from_seconds(50);

        let info = mock_info("voter", &[]);
        let msg = ExecuteMsg::VoteProposal {
            app_id: 1,
            proposal_id: 23,
            extended_pair: 3,
        };
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();
        match res {
            ContractError::CustomError { .. } => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn bribe_proposal_invalid_request() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // * Proposal does not exist
        bribe_proposal(deps.as_mut(), env.clone(), info.clone(), 24, 0).unwrap_err();

        // Create a proposal
        let proposal = Proposal {
            app_id: 1,
            voting_start_time: Timestamp::from_seconds(7),
            voting_end_time: Timestamp::from_seconds(10),
            extended_pair: vec![14, 38],
            emission_completed: false,
            rebase_completed: false,
            foundation_emission_completed: false,
            emission_distributed: 0,
            rebase_distributed: 0,
            foundation_distributed: 0,
            total_surplus: coin(0, DENOM),
            total_voted_weight: 12,
            height: 50,
        };
        PROPOSAL.save(deps.as_mut().storage, 23, &proposal).unwrap();

        // * Invalid extended pair
        bribe_proposal(deps.as_mut(), env.clone(), info.clone(), 1, 12).unwrap_err();

        // * Proposal completed voting period.
        // Set current block time past the voting_end_time
        env.block.time = Timestamp::from_seconds(50);

        bribe_proposal(deps.as_mut(), env.clone(), info.clone(), 23, 54).unwrap_err();
    }

    #[test]
    fn emission_invalid_request() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(8);
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // * Only admin can raise the request
        emission(deps.as_mut(), env.clone(), info, 12).unwrap_err();

        // Create a proposal
        let proposal = Proposal {
            app_id: 1,
            voting_start_time: Timestamp::from_seconds(7),
            voting_end_time: Timestamp::from_seconds(10),
            extended_pair: vec![],
            emission_completed: false,
            rebase_completed: false,
            foundation_emission_completed: false,
            emission_distributed: 0,
            rebase_distributed: 0,
            foundation_distributed: 0,
            total_surplus: coin(0, DENOM),
            total_voted_weight: 12,
            height: 50,
        };
        let proposal_id = 23u64;
        PROPOSAL
            .save(deps.as_mut().storage, proposal_id, &proposal)
            .unwrap();

        let info = mock_info("admin", &[]);
        // * Contract still in voting period
        let res = emission(deps.as_mut(), env.clone(), info.clone(), proposal_id).unwrap_err();
        match res {
            ContractError::CustomError { val }
                if val
                    == "Proposal Voting Period not ended to execute emission for the proposal"
                        .to_string() => {}
            e => panic!("{:?}", e),
        };

        // * Emission already completed
        env.block.time = Timestamp::from_seconds(11);
        let info = mock_info("admin", &[]);

        let res = emission(deps.as_mut(), env.clone(), info.clone(), proposal_id).unwrap_err();
        match res {
            ContractError::CustomError { val }
                if val == "Emission already completed".to_string() => {}
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn claim_rewards_invalid_request() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        // !------- Incomplete -------!
    }

    #[test]
    fn rebase_invalid_request() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);
        let proposal_id = 12;
        let app_id = 1;

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg).unwrap();

        // * Request from non-admin results in err
        let res = calculate_rebase_reward(deps.as_mut(), env.clone(), info, proposal_id, app_id)
            .unwrap_err();
        match res {
            ContractError::CustomError { val } if val == "Unauthorized".to_string() => {}
            e => panic!("{:?}", e),
        };

        let info = mock_info("admin", &[]);
        // * Invalid proposal
        let res = calculate_rebase_reward(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            proposal_id,
            app_id,
        )
        .unwrap_err();

        // * No locked users
        LOCKINGADDRESS
            .save(deps.as_mut().storage, app_id, &vec![])
            .unwrap();
        let res = calculate_rebase_reward(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            proposal_id,
            app_id,
        )
        .unwrap_err();
        match res {
            ContractError::CustomError { val }
                if val == "No locked users to rebase".to_string() => {}
            e => panic!("{:?}", e),
        };

        // Create a proposal
        let proposal = Proposal {
            app_id: 1,
            voting_start_time: Timestamp::from_seconds(7),
            voting_end_time: Timestamp::from_seconds(10),
            extended_pair: vec![],
            emission_completed: false,
            rebase_completed: true,
            foundation_emission_completed: false,
            emission_distributed: 0,
            rebase_distributed: 0,
            foundation_distributed: 0,
            total_surplus: coin(0, DENOM),
            total_voted_weight: 12,
            height: 50,
        };
        PROPOSAL
            .save(deps.as_mut().storage, proposal_id, &proposal)
            .unwrap();

        // * Rebase completed
        let res = calculate_rebase_reward(deps.as_mut(), env.clone(), info, proposal_id, app_id)
            .unwrap_err();
        match res {
            ContractError::CustomError { val } if val == "Rebase already completed".to_string() => {
            }
            e => panic!("{:?}", e),
        };
    }

    #[test]
    fn emission_foundation_invalid_request() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);
        let proposal_id = 12;
        let app_id = 1;

        // Initialize
        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg).unwrap();

        // * Invalid request by non admin
        let info = mock_info("admin", &[]);
        let res =
            emission_foundation(deps.as_mut(), env.clone(), info.clone(), proposal_id).unwrap_err();
        match res {
            ContractError::CustomError { val } if val == "Unauthorized".to_string() => {}
            e => panic!("{:?}", e),
        };

        // Create a proposal
        let mut proposal = Proposal {
            app_id: 1,
            voting_start_time: Timestamp::from_seconds(7),
            voting_end_time: Timestamp::from_seconds(10),
            extended_pair: vec![],
            emission_completed: false,
            rebase_completed: false,
            foundation_emission_completed: false,
            emission_distributed: 0,
            rebase_distributed: 0,
            foundation_distributed: 0,
            total_surplus: coin(0, DENOM),
            total_voted_weight: 12,
            height: 50,
        };
        PROPOSAL
            .save(deps.as_mut().storage, proposal_id, &proposal)
            .unwrap();

        // * Emmission not calculated
        let res =
            emission_foundation(deps.as_mut(), env.clone(), info.clone(), proposal_id).unwrap_err();
        match res {
            ContractError::CustomError { val } if val == "Emission calculation did not take place to initiate foundation calculation" => {}
            e => panic!("{:?}", e),
        };

        // * Foundation emission already completed
        proposal.foundation_emission_completed = true;
        PROPOSAL
            .save(deps.as_mut().storage, proposal_id, &proposal)
            .unwrap();

        let res =
            emission_foundation(deps.as_mut(), env.clone(), info.clone(), proposal_id).unwrap_err();
        match res {
            ContractError::CustomError { val } if val == "Emission already distributed" => {}
            e => panic!("{:?}", e),
        };

        // * Empty foundation addr vector
        let res =
            emission_foundation(deps.as_mut(), env.clone(), info.clone(), proposal_id).unwrap_err();
        match res {
            ContractError::CustomError { val } if val == "No foundation address found" => {}
            e => panic!("{:?}", e),
        };
    }
}
