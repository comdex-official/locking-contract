use crate::error::ContractError;
use crate::helpers::{
    get_token_supply, query_app_exists, query_extended_pair_by_app, query_get_asset_data,
    query_surplus_reward, query_whitelisted_asset,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use crate::state::{
    Emission, LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, STATE,
    SUPPLY, TOKENS, VTOKENS,
};
use crate::state::{
    Proposal, Vote, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, COMPLETEDPROPOSALS, EMISSION,
    MAXPROPOSALCLAIMED, PROPOSAL, PROPOSALCOUNT, PROPOSALVOTE, VOTERSPROPOSAL, VOTERS_VOTE,
    VOTINGPERIOD,
};
use comdex_bindings::{ComdexMessages, ComdexQuery};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, BankMsg, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, QueryRequest,
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
    let state = State {
        t1: msg.t1,
        t2: msg.t2,
        t3: msg.t3,
        t4: msg.t4,
        num_tokens: 0,
        vesting_contract: msg.vesting_contract,
        foundation_addr: msg.foundation_addr,
        foundation_percentage: msg.foundation_percentage,
        surplus_asset_id: msg.surplus_asset_id,
        voting_period: msg.voting_period,
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    VOTINGPERIOD.save(deps.storage, &msg.voting_period)?;
    EMISSION.save(deps.storage, msg.emission.app_id, &msg.emission)?;
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

        ExecuteMsg::Withdraw {
            denom,
            lockingperiod,
        } => handle_withdraw(deps, env, info, denom, lockingperiod),

        ExecuteMsg::Transfer {
            recipent,
            locking_period,
            denom,
        } => handle_transfer(deps, env, info, recipent, locking_period, denom),
        ExecuteMsg::FoundationRewards { proposal_id } => {
            emission_foundation(deps, env, info, proposal_id)
        }
        _ => panic!("Not implemented"),
    }
}

pub fn emission_foundation(
    deps: DepsMut<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    // check emission already compluted and executed
    if !proposal.emission_completed {
        return Err(ContractError::CustomError {
            val: "Emission caluclation did not take place to initiate rebase calculation"
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
    let foundation_emission = proposal.foundation_distributed;
    //// addr binding pending
    let emission_msg = ComdexMessages::MsgFoundationEmission {
        app_id: proposal.app_id,
        amount: foundation_emission,
        foundation_address: foundation_addr,
    };
    proposal.foundation_emission_completed = true;
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;

    Ok(Response::new()
        .add_messages(vec![emission_msg])
        .add_attribute("action", "lock")
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
        env,
        locking_period.clone(),
        period,
        weight,
        funds.clone(),
    )?;

    // Loads the NFT if present else None
    let nft = TOKENS.may_load(deps.storage, sender.clone())?;

    match nft {
        Some(mut token) => {
            // If NFT exists then lock new vtokens
            token.vtokens.push(new_vtoken.clone());

            TOKENS.save(deps.storage, sender.clone(), &token)?;
        }
        None => {
            // Create a new NFT
            state.num_tokens += 1;

            let mut new_nft = TokenInfo {
                owner: sender.clone(),
                vtokens: vec![],
                token_id: state.num_tokens,
            };

            new_nft.vtokens.push(new_vtoken.clone());
            TOKENS.save(deps.storage, sender.clone(), &new_nft)?;
        }
    };

    // Update VTOKENS
    VTOKENS.update(
        deps.storage,
        (sender.clone(), &funds.denom),
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

/// Handles the withdrawal of tokens after completion of locking period.
pub fn handle_withdraw(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    denom: String,
    locking_period: LockingPeriod,
) -> Result<Response<ComdexMessages>, ContractError> {
    if info.funds.len() != 0 {
        return Err(ContractError::FundsNotAllowed {});
    }

    // Load the token
    let vtokens = VTOKENS.may_load(deps.storage, (info.sender.clone(), &denom))?;

    if let None = vtokens {
        return Err(ContractError::NotFound {
            msg: format!("No tokens found for {:?}", denom),
        });
    }

    let mut vtokens_denom = vtokens.unwrap();

    // Retrive unlocked tokens with the given locking period
    let vtokens: Vec<(usize, &Vtoken)> = vtokens_denom
        .iter()
        .enumerate()
        .filter(|s| s.1.period == locking_period && s.1.end_time < env.block.time)
        .collect();

    // No unlocked tokens
    if vtokens.is_empty() {
        return Err(ContractError::NotFound {
            msg: format!("No unlocked tokens found for {:?}", locking_period),
        });
    }

    // Calculate total withdrawable amount and remove the corresponding VToken
    let mut withdrawable = 0u128;
    let mut indices: Vec<usize> = vec![];
    for (index, vtoken) in vtokens {
        withdrawable += vtoken.token.amount.u128();
        indices.push(index);
    }
    for index in indices {
        vtokens_denom.remove(index);
    }
    // Update VTOKENS
    if vtokens_denom.is_empty() {
        VTOKENS.remove(deps.storage, (info.sender.clone(), &denom));
    } else {
        VTOKENS.save(deps.storage, (info.sender.clone(), &denom), &vtokens_denom)?;
    };

    // Update nft
    let mut nft = TOKENS.load(deps.as_ref().storage, info.sender.clone())?;
    let denom_indicies: Vec<(usize, &Vtoken)> = nft
        .vtokens
        .iter()
        .enumerate()
        .filter(|el| {
            el.1.token.denom == denom
                && el.1.period == locking_period
                && el.1.end_time < env.block.time
        })
        .collect();

    // Remove the unlocked tokens
    let mut indices: Vec<usize> = vec![];
    for (index, _) in denom_indicies.into_iter() {
        indices.push(index);
    }
    for index in indices {
        nft.vtokens.remove(index);
    }
    TOKENS.save(deps.storage, info.sender.clone(), &nft)?;

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
    _env: Env,
    info: MessageInfo,
    recipient: String,
    locking_period: LockingPeriod,
    denom: String,
) -> Result<Response<ComdexMessages>, ContractError> {
    if info.funds.len() != 0 {
        return Err(ContractError::FundsNotAllowed {});
    }
    let recipient = deps.api.addr_validate(&recipient)?;

    // Load the sender denom that needs to be transfered
    let sender_vtokens = VTOKENS.may_load(deps.storage, (info.sender.clone(), &denom))?;

    if let None = sender_vtokens {
        return Err(ContractError::NotFound {
            msg: format!("No tokens found for {:?}", denom),
        });
    }
    let sender_denom_vtokens = sender_vtokens.unwrap();

    // Load tokens with given locking period
    let sender_vtokens_to_transfer: Vec<Vtoken> = sender_denom_vtokens
        .clone()
        .into_iter()
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
        // Remaining vtokens are saved to sender's VTOKENS
        let sender_vtokens_remaining: Vec<Vtoken> = sender_denom_vtokens
            .into_iter()
            .filter(|el| !(el.period == locking_period))
            .collect();

        if sender_vtokens_remaining.is_empty() {
            VTOKENS.remove(deps.storage, (info.sender.clone(), &denom));
        } else {
            VTOKENS.save(
                deps.storage,
                (info.sender.clone(), &denom),
                &sender_vtokens_remaining,
            )?;
        }
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
        recipient_vtokens.extend(sender_vtokens_to_transfer);

        VTOKENS.save(
            deps.storage,
            (recipient.clone(), &denom),
            &recipient_vtokens,
        )?;
    }

    // Load sender's nft
    let mut sender_nft = TOKENS.load(deps.as_ref().storage, info.sender.clone())?;

    // Retrieve tokens with denom and locking period
    let sender_nft_denom_vtoken = sender_nft
        .vtokens
        .clone()
        .into_iter()
        .filter(|el| el.token.denom == denom && el.period == locking_period);

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

    recipient_nft.vtokens.extend(sender_nft_denom_vtoken);

    TOKENS.save(deps.storage, recipient.clone(), &recipient_nft)?;

    let sender_nft_denom_remaining: Vec<Vtoken> = sender_nft
        .vtokens
        .into_iter()
        .filter(|el| !(el.token.denom == denom && el.period == locking_period))
        .collect();

    sender_nft.vtokens = sender_nft_denom_remaining;

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
) -> Result<Response<ComdexMessages>, ContractError> {
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
    let found = query_whitelisted_asset(deps.as_ref(), bribe_coin.denom.clone())?;
    if !found {
        return Err(ContractError::CustomError {
            val: String::from("Asset not whitelisted on chain"),
        });
    }
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
    Ok(Response::new().add_attribute("method", "bribe"))
}

pub fn claim_rewards(
    mut deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    //Check active proposal

    let max_proposal_claimed = MAXPROPOSALCLAIMED
        .load(deps.storage, (app_id, info.sender.clone()))
        .unwrap_or_default();

    let all_proposals = COMPLETEDPROPOSALS.load(deps.storage, app_id)?;
    calculate_rebase_reward(
        deps.branch(),
        env.clone(),
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    )?;

    let bribe_coins = calculate_bribe_reward(
        deps.as_ref(),
        env.clone(),
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    )?;
    let surplus_share = calculate_surplus_reward(
        deps.as_ref(),
        env.clone(),
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    )?;

    MAXPROPOSALCLAIMED.save(
        deps.storage,
        (app_id, info.sender.clone()),
        all_proposals.last().unwrap(),
    )?;
    Ok(Response::new()
        .add_attribute("method", "voted for proposal")
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: bribe_coins,
        })
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![surplus_share],
        }))
}

pub fn calculate_bribe_reward(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    max_proposal_claimed: u64,
    all_proposals: Vec<u64>,
    _app_id: u64,
) -> Result<Vec<Coin>, ContractError> {
    //check if active proposal
    let mut bribe_coins: Vec<Coin> = vec![];
    for proposalid in all_proposals.clone() {
        if proposalid <= max_proposal_claimed {
            continue;
        }
        let vote = VOTERSPROPOSAL.load(deps.storage, (info.sender.clone(), proposalid))?;
        let proposal1 = PROPOSAL.load(deps.storage, proposalid)?;
        if vote.bribe_claimed {
            return Err(ContractError::CustomError {
                val: "Bribe Already Claimed".to_string(),
            });
        }
        let total_vote_weight = PROPOSALVOTE
            .load(deps.storage, (proposal1.app_id, vote.extended_pair))?
            .u128();
        let total_bribe =
            BRIBES_BY_PROPOSAL.load(deps.storage, (proposal1.app_id, vote.extended_pair))?;

        let mut claimable_bribe: Vec<Coin> = vec![];

        for coin in total_bribe.clone() {
            let claimable_amount = (vote.vote_weight / total_vote_weight) * coin.amount.u128();
            let claimable_coin = Coin {
                amount: Uint128::from(claimable_amount),
                denom: coin.denom,
            };
            claimable_bribe.push(claimable_coin);
        }
        for bribr_deposited in claimable_bribe.clone() {
            match bribe_coins
                .iter_mut()
                .find(|ref p| bribr_deposited.denom == p.denom)
            {
                Some(pivot) => {
                    pivot.denom = bribr_deposited.denom;
                    pivot.amount += bribr_deposited.amount;
                }
                None => {
                    bribe_coins.push(bribr_deposited);
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
    max_proposal_claimed: u64,
    all_proposals: Vec<u64>,
    app_id: u64,
) -> Result<(), ContractError> {
    let mut total_rebase_amount: u128 = 0;
    for proposalid in all_proposals.clone() {
        if proposalid <= max_proposal_claimed {
            continue;
        }
        let proposal = PROPOSAL.load(deps.storage, proposalid)?;
        total_rebase_amount += proposal.rebase_distributed;
    }

    let app_response = query_app_exists(deps.as_ref(), app_id)?;
    let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;
    let vtokens = VTOKENS.load(deps.storage, (info.sender.clone(), &gov_token_denom))?;
    let supply = SUPPLY.load(deps.storage, &gov_token_denom)?;
    let total_locked: u128 = supply.vtoken;
    //// get rebase amount per period
    let mut locked_t1: u128 = 0;
    let mut locked_t2: u128 = 0;
    let mut locked_t3: u128 = 0;
    let mut locked_t4: u128 = 0;

    for vtoken in vtokens.clone() {
        match vtoken.period {
            LockingPeriod::T1 => locked_t1 += vtoken.vtoken.amount.u128(),
            LockingPeriod::T2 => locked_t2 += vtoken.vtoken.amount.u128(),
            LockingPeriod::T3 => locked_t3 += vtoken.vtoken.amount.u128(),
            LockingPeriod::T4 => locked_t4 += vtoken.vtoken.amount.u128(),
        }
    }

    let total_share = locked_t1 + locked_t2 + locked_t3 + locked_t4;

    //// lock in t1
    let lock_amount_t1 = (locked_t1 / total_share) * (total_rebase_amount / total_locked);
    let fund_t1 = Coin {
        amount: Uint128::from(lock_amount_t1),
        denom: gov_token_denom.clone(),
    };
    lock_funds(
        deps.branch(),
        env.clone(),
        app_id,
        info.sender.clone(),
        fund_t1,
        LockingPeriod::T1,
    )?;

    let lock_amount_t2 = (locked_t2 / total_share) * (total_rebase_amount / total_locked);
    let fund_t2 = Coin {
        amount: Uint128::from(lock_amount_t2),
        denom: gov_token_denom.clone(),
    };
    lock_funds(
        deps.branch(),
        env.clone(),
        app_id,
        info.sender.clone(),
        fund_t2,
        LockingPeriod::T2,
    )?;

    let lock_amount_t3 = (locked_t3 / total_share) * (total_rebase_amount / total_locked);
    let fund_t3 = Coin {
        amount: Uint128::from(lock_amount_t3),
        denom: gov_token_denom.clone(),
    };
    lock_funds(
        deps.branch(),
        env.clone(),
        app_id,
        info.sender.clone(),
        fund_t3,
        LockingPeriod::T3,
    )?;

    let lock_amount_t4 = (locked_t4 / total_share) * (total_rebase_amount / total_locked);
    let fund_t4 = Coin {
        amount: Uint128::from(lock_amount_t4),
        denom: gov_token_denom.clone(),
    };
    lock_funds(
        deps.branch(),
        env.clone(),
        app_id,
        info.sender.clone(),
        fund_t4,
        LockingPeriod::T4,
    )?;

    Ok(())
}

pub fn calculate_surplus_reward(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    max_proposal_claimed: u64,
    all_proposals: Vec<u64>,
    app_id: u64,
) -> Result<Coin, ContractError> {
    let mut total_surplus_available: Coin = Coin {
        amount: Uint128::from(0 as u32),
        denom: "null".to_string(),
    };
    for proposalid in all_proposals.clone() {
        if proposalid <= max_proposal_claimed {
            continue;
        }
        let proposal = PROPOSAL.load(deps.storage, proposalid)?;
        total_surplus_available.amount += proposal.total_surplus.amount;
        total_surplus_available.denom = proposal.total_surplus.denom;
    }

    let app_response = query_app_exists(deps, app_id)?;
    let gov_token_denom = query_get_asset_data(deps, app_response.gov_token_id)?;
    let vtokens = VTOKENS.load(deps.storage, (info.sender.clone(), &gov_token_denom))?;
    let supply = SUPPLY.load(deps.storage, &gov_token_denom)?;
    let total_locked: u128 = supply.vtoken;
    //// get rebase amount per period
    let mut locked: u128 = 0;

    for vtoken in vtokens.clone() {
        locked += vtoken.vtoken.amount.u128();
    }

    let share = locked.div(total_locked);
    let claim_coin = Coin {
        amount: Uint128::from(share),
        denom: gov_token_denom,
    };

    Ok(claim_coin)
}

pub fn emission(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    _info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    // check if already emission executed
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time > env.block.time.seconds() {
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
    let vtokens = SUPPLY.load(deps.storage, &gov_token_denom)?;
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
    for i in 0..ext_pair.len() {
        let vote = PROPOSALVOTE
            .load(deps.storage, (app_id, ext_pair[i]))
            .unwrap_or_default();
        votes.push(vote);
    }

    //// UPDATE Foundation Nodes Share
    proposal.foundation_distributed = (state.foundation_percentage.mul(effective_emission)).u128();
    //// Update proposal Emission State
    proposal.emission_completed = true;
    proposal.emission_distributed = effective_emission.u128();

    //// UPDATE REBASE AMOUNT
    proposal.rebase_distributed = (reward_emision.mul(percentage_locked)).u128();

    //// EMISSION Data Update
    emission.rewards_pending -= effective_emission.u128();
    emission.distributed_rewards += effective_emission.u128();

    let surplus = query_surplus_reward(deps.as_ref(), app_id, state.surplus_asset_id)?;
    proposal.total_surplus = surplus.clone();
    EMISSION.save(deps.storage, proposal.app_id, &emission)?;
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    let mut msg: Vec<ComdexMessages> = vec![];
    let emission_msg = ComdexMessages::MsgEmissionRewards {
        app_id: app_id,
        emission_amount: effective_emission.u128(),
        extended_pair: proposal.extended_pair,
        voting_ratio: votes,
    };
    let rebase_msg = ComdexMessages::MsgRebaseMint {
        app_id: app_id,
        amount: proposal.rebase_distributed,
        contract_addr: env.contract.address.clone(),
    };
    let surplus_msg = ComdexMessages::MsgGetSurplusFund {
        app_id: app_id,
        asset_id: state.surplus_asset_id,
        contract_addr: env.contract.address,
        amount: surplus.clone(),
    };
    msg.push(emission_msg);
    msg.push(rebase_msg);

    if surplus.amount != Uint128::new(0) {
        msg.push(surplus_msg);
    }

    let mut all_proposals = COMPLETEDPROPOSALS.load(deps.storage, app_id)?;
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
    //// check if already voted for proposal
    let has_voted = VOTERS_VOTE
        .load(deps.storage, (info.sender.clone(), proposal_id))
        .unwrap_or_default();
    if has_voted {
        return Err(ContractError::CustomError {
            val: "Already voted for the proposal".to_string(),
        });
    }

    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;

    // Check if proposal in voting period
    if proposal.voting_end_time < env.block.time.seconds() {
        return Err(ContractError::CustomError {
            val: "Proposal Voting Period Ended".to_string(),
        });
    }
    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    let extended_pairs = proposal.extended_pair.clone();

    // check if ext_pair param exist in extended pair list to vote for

    match extended_pairs.binary_search(&extended_pair) {
        Ok(_) => (),
        Err(_) => {
            return Err(ContractError::CustomError {
                val: "Invalid Extended pair".to_string(),
            })
        }
    }

    //balance of owner for the for denom for voting

    let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;

    if gov_token_denom.is_empty() || app_response.gov_token_id == 0 {
        return Err(ContractError::CustomError {
            val: "Gov token not found for the app".to_string(),
        });
    }
    let vtokens = VTOKENS.load(deps.storage, (info.sender.clone(), &gov_token_denom))?;

    // calculate voting power for the the proposal
    let mut vote_power: u128 = 0;

    for vtoken in vtokens.clone() {
        proposal.total_voted_weight += vtoken.vtoken.amount.u128();
        vote_power += vtoken.vtoken.amount.u128();
    }

    // Update proposal Vote for an app

    let proposal_vote = PROPOSALVOTE
        .load(deps.storage, (proposal_id, extended_pair))
        .unwrap_or_default();

    PROPOSALVOTE.save(
        deps.storage,
        (proposal_id, extended_pair),
        &(Uint128::from(vote_power) + proposal_vote),
    )?;

    // update proposal
    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    let vote = Vote {
        app_id: app_id,
        extended_pair: extended_pair,
        vote_weight: vote_power,
        bribe_claimed: false,
    };
    VOTERSPROPOSAL.save(deps.storage, (info.sender, proposal_id), &vote)?;

    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn raise_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    _info: MessageInfo,
    app_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    //check if app exist
    query_app_exists(deps.as_ref(), app_id)?;
    //get ext pairs vec from app
    let ext_pairs = query_extended_pair_by_app(deps.as_ref(), app_id)?;

    //check no proposal active for app
    let current_app_proposal = match APPCURRENTPROPOSAL.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => 0,
    };

    // if proposal already exist , check if whether it is in voting period
    // proposal cannot be raised until current proposal voting time is ended
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
        extended_pair: ext_pairs,
        emission_completed: false,
        rebase_completed: false,
        emission_distributed: 0,
        rebase_distributed: 0,
        total_voted_weight: 0,
        foundation_emission_completed: false,
        foundation_distributed: 0,
        total_surplus: Coin {
            amount: Uint128::from(0 as u32),
            denom: "nodenom".to_string(),
        },
        height: env.block.height,
    };
    let current_proposal = PROPOSALCOUNT.load(deps.storage).unwrap_or_default();
    PROPOSALCOUNT.save(deps.storage, &(current_proposal + 1))?;
    APPCURRENTPROPOSAL.save(deps.storage, app_id, &(current_proposal + 1))?;
    PROPOSAL.save(deps.storage, current_proposal + 1, &proposal)?;
    Ok(Response::new().add_attribute("method", "reset"))
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    let ver = cw2::get_contract_version(deps.storage)?;
    // ensure we are migrating from an allowed contract
    if ver.contract != CONTRACT_NAME {
        return Err(StdError::generic_err("Can only upgrade from same type").into());
    }
    // note: better to do proper semver compare, but string compare *usually* works
    if ver.version >= CONTRACT_VERSION.to_string() {
        return Err(StdError::generic_err("Cannot upgrade from a newer version").into());
    }

    // set the new version
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // do any desired state migrations...

    Ok(Response::default())
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use super::*;
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{coin, coins, Addr, CosmosMsg, OwnedDeps, StdError};

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
            foundation_addr: vec![Addr::unchecked("vesting_contract")],
            foundation_percentage: Decimal::new(Uint128::from(2 as u32)),
            surplus_asset_id: 3,
            emission: Emission {
                app_id: 1,
                total_rewards: 200000,
                rewards_pending: 200000,
                emmission_rate: Decimal::new(Uint128::from(2 as u64)),
                distributed_rewards: 123333,
            },
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

        // Check correct updation in nft
        let nft = TOKENS
            .load(deps.as_ref().storage, owner_addr.clone())
            .unwrap();
        assert_eq!(nft.vtokens.len(), 2);
        assert_eq!(nft.vtokens[0].token.amount.u128(), 100u128);
        assert_eq!(nft.vtokens[0].vtoken.amount.u128(), 25u128);
        assert_eq!(nft.vtokens[0].start_time, old_start_time);
        assert_eq!(
            nft.vtokens[0].end_time,
            old_start_time.plus_seconds(imsg.t1.period)
        );
        assert_eq!(nft.vtokens[0].period, LockingPeriod::T1);
        assert_eq!(nft.vtokens[0].status, Status::Locked);

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

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        env.block.time = env.block.time.plus_seconds(imsg.t1.period + 1u64);

        // Withdrawing 10 Tokens
        let info = mock_info("owner", &[]);
        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            DENOM.to_string(),
            LockingPeriod::T1,
        )
        .unwrap();
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
        let vtoken = VTOKENS
            .load(&deps.storage, (info.sender.clone(), DENOM))
            .unwrap_err();
        // assert_eq!(vtoken.len(), 1);
        // assert_eq!(vtoken[0].token.amount.u128(), 90u128);
        // let vtoken_balance = Uint128::from(25u64).sub(Uint128::from(10u64) * imsg.t1.weight);
        // assert_eq!(vtoken[0].vtoken.amount.u128(), vtoken_balance.u128());
        // assert_eq!(vtoken[0].status, Status::Unlocked);

        // Check correct update in nft
        let nft = TOKENS.load(deps.as_ref().storage, owner.clone()).unwrap();
        assert_eq!(nft.vtokens.len(), 0);
        // assert_eq!(nft.vtokens[0].token.amount.u128(), 90u128);
        // assert_eq!(nft.vtokens[0].vtoken.amount.u128(), vtoken_balance.u128());
        // assert_eq!(nft.vtokens[0].status, Status::Unlocked);
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
            LockingPeriod::T1,
        )
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

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            DENOM.to_string(),
            LockingPeriod::T1,
        )
        .unwrap_err();
        // match res {
        //     ContractError::NotUnlocked { .. } => {}
        //     e => panic!("{:?}", e),
        // };
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

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let info = mock_info("owner", &[]);
        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            DENOM.to_string(),
            LockingPeriod::T1,
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
        };

        let owner = Addr::unchecked("owner");
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let info = mock_info("owner", &[]);
        let res = handle_withdraw(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            "DNM1".to_string(),
            LockingPeriod::T1,
        )
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

        // Check correct update in sender nft
        let sender_nft = TOKENS.load(deps.as_ref().storage, owner.clone()).unwrap();
        assert_eq!(sender_nft.vtokens.len(), 0);

        // Check correct update in recipient nft
        let recipient_nft = TOKENS
            .load(deps.as_ref().storage, recipient.clone())
            .unwrap();
        assert_eq!(recipient_nft.owner, recipient.clone());
        assert_eq!(recipient_nft.vtokens.len(), 1);
        assert_eq!(recipient_nft.vtokens[0], locked_vtokens[0]);
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
        assert_eq!(sender_nft.vtokens.len(), 0);

        // Check correct update in recipient nft
        let recipient_nft = TOKENS
            .load(deps.as_ref().storage, recipient.clone())
            .unwrap();
        assert_eq!(recipient_nft.owner, recipient.clone());
        assert_eq!(recipient_nft.vtokens.len(), 2);
        assert_eq!(recipient_nft.vtokens[0].token.amount.u128(), 100);
        assert_eq!(recipient_nft.vtokens[0].token.denom, denom2.to_string());
        assert_eq!(recipient_nft.vtokens[1], locked_vtokens[0]);
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
}
