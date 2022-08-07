#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult,Coin,Storage,Decimal,Uint128,Addr,Timestamp,BankMsg};
use cw2::set_contract_version;
use std::ops::{AddAssign, Sub, SubAssign, Div};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{  APPCURRENTPROPOSAL,Proposal,VOTINGPERIOD,PROPOSALCOUNT,PROPOSAL,PROPOSALVOTE,BRIBES_BY_PROPOSAL,EMISSION};
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:{{project-name}}";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
use comdex_bindings::{ComdexMessages, ComdexQuery};
use crate::helpers::{query_app_exists,query_get_asset_data};
use crate::state::{
    LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, LOCKED, STATE,
    SUPPLY, TOKENS, UNLOCKED, UNLOCKING, VTOKENS,
};
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        t1: msg.t1,
        t2: msg.t2,
        t3: msg.t3,
        t4: msg.t4,
        unlock_period: msg.unlock_period,
        num_tokens: 0,
    };

    STATE.save(deps.storage, &state)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    VOTINGPERIOD.save(deps.storage, &msg.voting_period)?;
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender)
        )
}



#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::VoteProposal{app_id,proposal_id,extended_pair} => vote_proposal(deps,env,info,app_id,proposal_id,extended_pair),
        ExecuteMsg::RaiseProposal { app_id } => raise_proposal(deps,env, info, app_id),
        ExecuteMsg::Bribe { proposal_id } => bribe_proposal(deps,env, info, proposal_id),
        ExecuteMsg::Emmission { proposal_id } => emission(deps,env, info, proposal_id),
        ExecuteMsg::Rebase { proposal_id } => emission(deps,env, info, proposal_id),
        ExecuteMsg::Lock {
            app_id,
            locking_period,
        } => handle_lock_nft(deps, env, info, app_id, locking_period),

        ExecuteMsg::Unlock { app_id, denom } => handle_unlock_nft(deps, env, info, app_id, denom),

        ExecuteMsg::Withdraw {
            app_id,
            denom,
            amount,
        } => withdraw(deps, &env, info, denom, amount),

    }
}


/// Lock the sent tokens and create corresponding vtokens
pub fn handle_lock_nft(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    locking_period: LockingPeriod,
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

    let mut state = STATE.load(deps.storage)?;

    // Load the locking period and weight
    let PeriodWeight { period, weight } = get_period(state.clone(), locking_period.clone())?;

    // Loads the NFT if present else None
    let nft = TOKENS.may_load(deps.storage, info.sender.clone())?;

    match nft {
        Some(mut token) => {
            let res: Vec<&Vtoken> = token
                .vtokens
                .iter()
                .filter(|s| s.token.denom == info.funds[0].denom && s.period == locking_period)
                .collect();

            if res.is_empty() {
                // !------- BUG -------!
                // !------- VTOKENS is not being updated -------!

                // create new token
                let new_vtoken = create_vtoken(
                    deps.storage,
                    &env,
                    &info,
                    locking_period.clone(),
                    period,
                    weight,
                )?;

                // Save updated nft
                token.vtokens.push(new_vtoken);
                TOKENS.save(deps.storage, info.sender.clone(), &token)?;
            } else {
                let mut vtoken = res[0].to_owned();

                if let Status::Locked = vtoken.status {
                    ()
                } else {
                    return Err(ContractError::NotLocked {});
                }

                let mut remaining: Vec<Vtoken> = token
                    .vtokens
                    .into_iter()
                    .filter(|s| {
                        !(s.token.denom == info.funds[0].denom && s.period == locking_period)
                    })
                    .collect();

                // Increase the token count
                vtoken.token.amount.add_assign(info.funds[0].amount.clone());

                // Increase the vtoken count
                vtoken
                    .vtoken
                    .amount
                    .add_assign(weight * info.funds[0].amount);

                // The new start time will be current block time, i.e. the old
                // tokens will also unlock with the new tokens.
                vtoken.start_time = env.block.time;
                vtoken.end_time = env.block.time.plus_seconds(period);

                remaining.push(vtoken);
                token.vtokens = remaining;

                TOKENS.save(deps.storage, info.sender.clone(), &token)?;
            }

            // LOCKED.save(deps.storage, info.sender.clone(), &locked_tokens)?;
            update_locked(
                deps.storage,
                info.sender.clone(),
                info.funds[0].denom.clone(),
                info.funds[0].amount,
                true,
            )?;
        }
        None => {
            // Create a new NFT
            state.num_tokens += 1;

            let mut new_nft = TokenInfo {
                owner: info.sender.clone(),
                vtokens: vec![],
                token_id: state.num_tokens,
            };

            // Create new Vtoken for new deposit
            let new_vtoken = create_vtoken(
                deps.storage,
                &env,
                &info,
                locking_period.clone(),
                period,
                weight,
            )?;

            VTOKENS.save(
                deps.storage,
                (info.sender.clone(), &info.funds[0].denom),
                &new_vtoken,
            )?;

            LOCKED.save(
                deps.storage,
                info.sender.clone(),
                &vec![info.funds[0].clone()],
            )?;

            new_nft.vtokens.push(new_vtoken);
            TOKENS.save(deps.storage, info.sender.clone(), &new_nft)?;
        }
    }

    Ok(Response::new()
        .add_attribute("action", "lock")
        .add_attribute("from", info.sender))
}

fn create_vtoken(
    storage: &mut dyn Storage,
    env: &Env,
    info: &MessageInfo,
    locking_period: LockingPeriod,
    period: u64,
    weight: Decimal,
) -> Result<Vtoken, ContractError> {
    // Create the vtoken
    let mut vdenom = String::from("v");
    vdenom.push_str(&info.funds[0].denom);

    let amount = weight * info.funds[0].amount;

    update_denom_supply(
        storage,
        vdenom.as_str(),
        amount.u128(),
        info.funds[0].amount.u128(),
    )?;

    Ok(Vtoken {
        token: info.funds[0].clone(),
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
    let quantity = quantity;
    let vdenom_supply = SUPPLY.may_load(storage, vdenom)?;
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

pub fn handle_unlock_nft(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    app_id: u64,
    denom: String,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    let mut Vtoken = VTOKENS.load(deps.storage, (info.sender, &denom)).unwrap();

    if Vtoken.status == Status::Unlocked {
        ContractError::AllreadyUnLocked {};
    }
    let t = Timestamp::from_seconds(state.unlock_period).seconds();

    if Vtoken.end_time < env.block.time
        && Vtoken.end_time.seconds() + state.unlock_period > env.block.time.seconds()
    {
        Vtoken.status = Status::Unlocking;
        // UNLOCKING.save(deps.storage, info.sender, data)
    } else if Vtoken.end_time.seconds() + state.unlock_period < env.block.time.seconds() {
        Vtoken.status = Status::Unlocked
    } else {
        ContractError::TimeNotOvered {};
    }

    Ok(Response::new().add_attribute("action", "unlock"))
}

pub fn withdraw(
    deps: DepsMut<ComdexQuery>,
    env: &Env,
    info: MessageInfo,
    denom: String,
    amount: u64,
) -> Result<Response, ContractError> {
    let mut Vtoken = VTOKENS
        .load(deps.storage, (info.sender.clone(), &denom))
        .unwrap();

    if Vtoken.status != Status::Unlocked {
        ContractError::NotUnlocked {};
    }

    if Vtoken.token.amount < Uint128::from(amount) {
        ContractError::InsufficientFunds {
            funds: Vtoken.token.amount.u128(),
        };
    }

    let withdraw_amount = Vtoken.token.amount.sub(Uint128::from(amount));
    Vtoken.token.amount -= Uint128::from(amount);
    VTOKENS.save(
        deps.storage,
        (info.sender.clone(), &info.funds[0].denom),
        &Vtoken,
    )?;
    let vtoken = VTOKENS.load(deps.storage, (info.sender.clone(), &info.funds[0].denom))?;
    if vtoken.token.amount.is_zero() {
        VTOKENS.remove(deps.storage, (info.sender.clone(), &denom));
    }

    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom,
                amount: withdraw_amount,
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



pub fn bribe_proposal(deps: DepsMut<ComdexQuery>,env:Env,info:MessageInfo, proposal_id:u64) -> Result<Response, ContractError> {
    //check if active proposal
    let proposal=PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time<env.block.time.seconds(){
        return Err(ContractError::CustomError { val: "Proposal Voting Period Ended".to_string() });
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

    let bribe_coin= info.funds[0].clone();
    let mut existing_bribes=BRIBES_BY_PROPOSAL.load(deps.storage, proposal_id)?; 
    let mut found=false;
    for mut coin in existing_bribes.clone(){
        if bribe_coin.denom==coin.denom
        {
            coin.amount+=bribe_coin.amount;
            found=true;
        }
    }
    if !found
    {
        existing_bribes.push(bribe_coin);
    }

    BRIBES_BY_PROPOSAL.save(deps.storage, proposal_id,&existing_bribes)?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn emission(deps: DepsMut<ComdexQuery>,env:Env,info:MessageInfo, proposal_id:u64) -> Result<Response, ContractError> {
    // check if already emission executed
    //check if active proposal
    let mut proposal=PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time>env.block.time.seconds(){
        return Err(ContractError::CustomError { val: "Proposal Voting Period not ended to execute emission for the proposal".to_string() });}
    let app_id=proposal.app_id;
    //check governance token via app_id
    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    let gov_token_id = app_response.gov_token_id;

    let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
    if gov_token_denom.is_empty() || gov_token_id == 0 {
        return Err(ContractError::CustomError { val: "Gov token not found".to_string() });
    }

    let vtokens=SUPPLY.load(deps.storage, &gov_token_denom)?;
    let total_token_locked=vtokens.token;
    let total_v_token=vtokens.vtoken;
    let percentage_locked= Decimal::raw(total_v_token).div(Decimal::raw(total_token_locked+total_v_token)) ;
    let emission=EMISSION.load(deps.storage,proposal.app_id)?;
    let reward_emision=Decimal::raw(emission.rewards_pending).checked_mul(emission.emmission_rate).unwrap();

    // mint and distribue to vault owner  based vote portion
    let ext_pair=proposal.extended_pair.clone();
    let mut votes:Vec<Uint128>=vec![];
    for i in 0..ext_pair.len(){
        let mut vote=PROPOSALVOTE.load(deps.storage, (app_id,ext_pair[i])).unwrap_or_default();
        votes.push(vote);
    }

    proposal.emission_completed=true;
    // bribe denom should be a single coin

    PROPOSAL.save(deps.storage, proposal_id, &proposal);
    Ok(Response::new().add_attribute("method", "voted for proposal"))

}

pub fn rebase(deps: DepsMut<ComdexQuery>,env:Env,info:MessageInfo, proposal_id:u64) -> Result<Response, ContractError> {
    //check if active proposal
    let proposal=PROPOSAL.load(deps.storage, proposal_id)?;
    // check emission already compluted and executed
    if !proposal.emission_completed
    {
        return Err(ContractError::CustomError { val: "Emission caluclation did not take place to initiate rebase calculation".to_string() });
    }
    if proposal.voting_end_time>env.block.time.seconds(){
        return Err(ContractError::CustomError { val: "proposal in voting period".to_string() });}
    let app_id=proposal.app_id;
    //check governance token via app_id
    let app_response = query_app_exists(deps.as_ref(), app_id)?;

    let gov_token_id = app_response.gov_token_id;

    let gov_token_denom = query_get_asset_data(deps.as_ref(), gov_token_id)?;
    if gov_token_denom.is_empty() || gov_token_id == 0 {
        return Err(ContractError::CustomError { val: "Invalid gov token".to_string() });
    }

    /// calculate rebase amount 
    let vtokens=SUPPLY.load(deps.storage, &gov_token_denom)?;
    let total_token_locked=vtokens.token;
    let total_v_token=vtokens.vtoken;
    let percentage_locked= (Decimal::raw(total_v_token).div(Decimal::raw(total_token_locked))).
    
    let total_token_locked=2000000;
    let total_v_token=50000;
    let percentage_locked:f64= 0.33;  //total_v_token/(total_token_locked+total_v_token);

    let emmission=700000;



    // bribe denom should be a single coin


    Ok(Response::new().add_attribute("method", "voted for proposal"))

}


pub fn vote_proposal(deps: DepsMut<ComdexQuery>,env:Env,info:MessageInfo, app_id : u64,proposal_id:u64,extended_pair:u64) -> Result<Response, ContractError> {
    //check if active proposal
    let proposal=PROPOSAL.load(deps.storage, proposal_id)?;
    if proposal.voting_end_time<env.block.time.seconds(){
        return Err(ContractError::CustomError { val: "Proposal Voting Period Ended".to_string() });
    }

    let app_response=query_app_exists(deps.as_ref(), app_id)?;
    
    let extended_pairs=proposal.extended_pair;
    let mut found_pair=false;
    for i in 0..extended_pairs.len(){
        if extended_pairs[i]==extended_pair
        {
            found_pair=true;
        }
    }
    //balance of owner for the for denom for voting

    let gov_token_denom = query_get_asset_data(deps.as_ref(), app_response.gov_token_id)?;
    if gov_token_denom.is_empty() || app_response.gov_token_id == 0 {
       return Err(ContractError::CustomError { val: "Gov token not found for the app".to_string() });
    }
    let vote_power=VTOKENS.load(deps.storage, (info.sender,&gov_token_denom))?;
    // get token owner balance
    if !found_pair
    {
        return Err(ContractError::CustomError { val: "Invalid Extended pair".to_string() });
    }
    PROPOSALVOTE.save(deps.storage, (proposal_id,extended_pair), &(vote_power.vtoken.amount))?;
    Ok(Response::new().add_attribute("method", "voted for proposal"))
}

pub fn raise_proposal(deps: DepsMut<ComdexQuery>,env: Env, info: MessageInfo, app_id : u64) -> Result<Response, ContractError> {
    //check if app exist
    let _app_response=query_app_exists(deps.as_ref(), app_id)?;
    //get ext pairs vec from app
    
    let response=vec![1,2,3];
    //get minimum deposit for proposal

    //check no proposal active for app
    let current_app_proposal = match APPCURRENTPROPOSAL.may_load(deps.storage, app_id)?
    {
        Some(val) => val,
        None => 0
    };
    
    if current_app_proposal!=0 {
        let proposal=PROPOSAL.load(deps.storage, current_app_proposal)?;
        if proposal.voting_end_time>env.block.time.seconds()
        {
            return Err(ContractError::CustomError { val: "Previous proposal in voting state for the app".to_string() })
        }
    }
    
    // set proposal data
    let voting_period=VOTINGPERIOD.load(deps.storage).unwrap_or_default();
    //update proposal maps
    let proposal=Proposal{
        app_id:app_id,
        voting_start_time:env.block.time.seconds(),
        voting_end_time:env.block.time.seconds()+voting_period,
        extended_pair: response,
        emission_completed: false,
        rebase_completed: false,
        emission_
    };
    let current_proposal=PROPOSALCOUNT.load(deps.storage).unwrap_or_default();
    PROPOSALCOUNT.save(deps.storage,&(current_proposal+1))?;
    APPCURRENTPROPOSAL.save(deps.storage, app_id, &(current_proposal+1))?;
    PROPOSAL.save(deps.storage, current_proposal+1, &proposal)?;
    Ok(Response::new().add_attribute("method", "reset"))
}

