#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, BankMsg, Coin, Decimal, DepsMut, Env, MessageInfo, QueryRequest,
    Response, Storage, Uint128, WasmQuery,
};
use cw2::set_contract_version;
use std::ops::{AddAssign, Div, Mul, Sub};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{
    CallType, Proposal, Vote, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, EMISSION, PROPOSAL,
    PROPOSALCOUNT, PROPOSALVOTE, VOTERSPROPOSAL, VOTERS_VOTE, VOTINGPERIOD,
};
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:{{project-name}}";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
use crate::helpers::{get_token_supply, query_app_exists, query_get_asset_data,query_extended_pair_by_app};
use crate::state::{
    LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, LOCKED, STATE,
    SUPPLY, TOKENS, UNLOCKED, VTOKENS,
};
use comdex_bindings::{ComdexMessages, ComdexQuery};

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
                vtoken.vtoken.amount.add_assign(weight * funds.amount);

                // The new start time will be current block time, i.e. the old
                // tokens will also unlock with the new tokens.
                if let CallType::UpdatePeriod = calltype.unwrap_or(CallType::UpdateAmount) {
                    vtoken.start_time = env.block.time;
                    vtoken.end_time = env.block.time.plus_seconds(period);
                }

                remaining.push(vtoken);
                token.vtokens = remaining;

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

    update_denom_supply(storage, vdenom.as_str(), amount.u128(), funds.amount.u128())?;

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
    vdenom: &str,
    vquantity: u128,
    quantity: u128,
) -> Result<(), ContractError> {
    // Load the total supply in the for the given denom
    let vdenom_supply = SUPPLY.may_load(storage, vdenom)?;
    // Create new struct if not present in SUPPLY
    let mut vdenom_supply_struct = vdenom_supply.unwrap_or(TokenSupply {
        token: 0,
        vtoken: 0,
    });

    vdenom_supply_struct.vtoken += vquantity;
    vdenom_supply_struct.token += quantity;

    SUPPLY.save(storage, vdenom, &vdenom_supply_struct)?;

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
    let mut vtokens = VTOKENS
        .load(deps.storage, (info.sender.clone(), &denom))
        .unwrap();

    // Retrive the locked tokens with the given locking period
    let locked_vtoken: Vec<(usize, &Vtoken)> = vtokens
        .iter()
        .enumerate()
        .filter(|s| s.1.period == locking_period)
        .collect();

    if locked_vtoken.is_empty() {
        return Err(ContractError::NotLocked {});
    }

    if locked_vtoken[0].1.token.amount < Uint128::from(amount) {
        ContractError::CustomError {
            val: "Cannot withdraw more than deposted".into(),
        };
    }

    let index = locked_vtoken[0].0;
    let vtoken = locked_vtoken[0].1.to_owned();

    let PeriodWeight { weight, period } =
        get_period(STATE.load(deps.as_ref().storage)?, locking_period.clone())?;

    // balance post withdrawal
    let token_balance = vtoken.token.amount.sub(Uint128::from(amount));
    let vtoken_balance = Uint128::from(amount) * weight;
    // Update token balance
    vtokens[index].token.amount = token_balance;
    vtokens[index].vtoken.amount = vtoken_balance;
    // Save the changes to VTOKENS
    VTOKENS.save(
        deps.storage,
        (info.sender.clone(), &info.funds[0].denom),
        &vtokens,
    )?;

    // !------- Need to update NFT.vtokens -------!
    let mut nft = TOKENS.load(deps.as_ref().storage, info.sender.clone())?;
    let denom_index: Vec<(usize, &Vtoken)> = nft
        .vtokens
        .iter()
        .enumerate()
        .filter(|el| el.1.token.denom == denom && el.1.period == locking_period.clone())
        .collect();

    let index = denom_index[0].0;
    nft.vtokens[index].token.amount = token_balance;
    nft.vtokens[index].vtoken.amount = vtoken_balance;

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

    // update bribe amount if already exist for same denom , else append in bribe vector
    let bribe_coin = info.funds[0].clone();

    let mut existing_bribes = BRIBES_BY_PROPOSAL.load(deps.storage, (proposal_id, extended_pair))?;
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
    let total_bribe = BRIBES_BY_PROPOSAL.load(deps.storage, (proposal.app_id, vote.extended_pair))?;

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
        let  vote = PROPOSALVOTE
            .load(deps.storage, (app_id, ext_pair[i]))
            .unwrap_or_default();
        votes.push(vote);
    }

    proposal.emission_completed = true;
    proposal.emission_distributed = effective_emission.u128();

    proposal.rebase_distributed=(reward_emision.mul(percentage_locked)).u128();
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

    if proposal.rebase_completed {
        return Err(ContractError::CustomError {
            val: "rebase already colmleted for the proposal"
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

    proposal.rebase_completed=true;


    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn claimrebase(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    //check if active proposal
    let mut proposal = PROPOSAL.load(deps.storage, proposal_id)?;
    // check emission already compluted and executed
    if !proposal.rebase_completed {
        return Err(ContractError::CustomError {
            val: "Rebase calculation".to_string(),
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
    let total_vtoken_weight = SUPPLY.load(deps.storage, &gov_token_denom)?;
    let rebase_amount = proposal.rebase_distributed;

    let vtokens = VTOKENS.load(deps.storage, (info.sender.clone(), &gov_token_denom))?;
        // get token owner balance
    
    let mut vote_power:u128=0;
    
    for vtoken in vtokens.clone()
    {
        proposal.total_voted_weight+=vtoken.vtoken.amount.u128();
        vote_power+=vtoken.vtoken.amount.u128();
    }    
    let rebase_claimable = (vote_power.div(total_vtoken_weight.vtoken)) * rebase_amount;

    PROPOSAL.save(deps.storage, proposal_id, &proposal)?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn vote_proposal(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    proposal_id: u64,
    extended_pair: u64,
) -> Result<Response, ContractError> {

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
    let app_response=query_app_exists(deps.as_ref(), app_id)?;
    
    let extended_pairs = proposal.extended_pair.clone();
    
    // check if ext_pair param exist in extended pair list to vote for

    match extended_pairs.binary_search(&extended_pair){
        Ok(u) => (),
        Err(_) => return Err(ContractError::CustomError {
            val: "Invalid Extended pair".to_string(),
        })
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
    let mut vote_power:u128=0;

    for vtoken in vtokens.clone()
    {
    proposal.total_voted_weight+=vtoken.vtoken.amount.u128();
    vote_power+=vtoken.vtoken.amount.u128();
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
) -> Result<Response, ContractError> {
    //check if app exist
    query_app_exists(deps.as_ref(), app_id)?;
    //get ext pairs vec from app
    let ext_pairs=query_extended_pair_by_app(deps.as_ref(),app_id)?;

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
    use cosmwasm_std::{coins, Addr, Api, OwnedDeps, Querier, StdError};

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

        // forward the time, inside 1 week
        // env.block.time.plus_seconds(100_000);

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
        let info = mock_info("owner", &coins(100, DENOM.to_string()));
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

    // #[test]
    // fn test_withdraw() {
    //     let mut deps = OwnedDeps {
    //         storage: MockStorage::default(),
    //         api: MockApi::default(),
    //         querier: MockQuerier::default(),
    //         custom_query_type: PhantomData,
    //     };
    //     let env = mock_env();
    //     let info = mock_info("sender", &coins(0, DENOM.to_string()));

    //     let imsg = init_msg();
    //     instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    //     let msg = ExecuteMsg::Lock {
    //         app_id: 12,
    //         locking_period: LockingPeriod::T1,
    //         calltype: None,
    //     };

    //     let info = mock_info("user1", &coins(100, DENOM.to_string()));

    //     let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

    //     let mut vtoken = VTOKENS
    //         .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
    //         .unwrap();
    //     vtoken.status = Status::Unlocked;

    //     assert_eq!(vtoken.token.denom, DENOM.to_string());
    //     assert_eq!(vtoken.status, Status::Unlocked);

    //     // Withdrawing 10 Tokens
    //     let err = withdraw(
    //         deps.as_mut(),
    //         &env,
    //         info.clone(),
    //         info.funds[0].denom.clone(),
    //         10,
    //         LockingPeriod::T1,
    //     );

    //     let mut _vtoken = VTOKENS
    //         .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
    //         .unwrap();

    //     assert_eq!(
    //         err,
    //         Ok(Response::new()
    //             .add_message(BankMsg::Send {
    //                 to_address: info.sender.to_string(),
    //                 amount: vec![_vtoken.token],
    //             })
    //             .add_attribute("action", "Withdraw")
    //             .add_attribute("Recipent", info.sender.clone()))
    //     );

    //     // Should left 100 - 10 = 90 tokens
    //     let mut _vtoken = VTOKENS
    //         .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
    //         .unwrap();
    //     let n: u64 = 90;
    //     assert_eq!(_vtoken.token.amount, Uint128::from(n));

    //     // Withdrawing All Tokens and Should remove the vtoken.
    //     let err = withdraw(
    //         deps.as_mut(),
    //         &env,
    //         info.clone(),
    //         info.funds[0].denom.clone(),
    //         90,
    //         LockingPeriod::T1,
    //     );

    //     let mut _vtoken = VTOKENS.load(&deps.storage, (info.sender, &info.funds[0].denom));
    //     assert_eq!(
    //         _vtoken,
    //         Err(StdError::NotFound {
    //             kind: "gov_locker::state::Vtoken".to_string()
    //         })
    //     );
    // }
}
