// !------- IssuedVtokens query not implemented-------!

use comdex_bindings::ComdexQuery;
use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Coin, Deps, Env, MessageInfo, StdError, StdResult,
};

use crate::msg::{IssuedNftResponse, QueryMsg,};
use crate::state::{TOKENS, VTOKENS,STATE,SUPPLY,APPCURRENTPROPOSAL,PROPOSAL,BRIBES_BY_PROPOSAL,VOTERS_VOTE,VOTERSPROPOSAL,MAXPROPOSALCLAIMED,COMPLETEDPROPOSALS,State,Vtoken,TokenSupply,Proposal,Vote};
use crate::contract::{calculate_bribe_reward};
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(
    deps: Deps<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::IssuedNft { address } => to_binary(&query_issued_nft(deps, env, info, address)?),
        QueryMsg::IssuedVtokens { address,denom } => {
            to_binary(&query_issued_vtokens(deps, env, info, address,denom)?)
        }
        QueryMsg::Supply { denom }=> to_binary(&query_issued_supply(deps, env, info,denom)?),
        QueryMsg::CurrentProposal{app_id} =>to_binary(&query_current_proposal(deps, env, info,app_id)?),
        QueryMsg::Proposal{proposal_id} =>to_binary(&query_proposal(deps, env, info,proposal_id)?),
        QueryMsg::BribeByProposal{proposal_id,app_id} =>to_binary(&query_bribe(deps, env, info,app_id,proposal_id)?),
        QueryMsg::Vote{proposal_id,address}=>to_binary(&query_vote(deps, env, info,address,proposal_id)?),
        QueryMsg::ClaimableBribe { address, app_id}=>to_binary(&query_bribe_eligible(deps, env, info,address,app_id)?),
        _ => panic!("Not implemented"),
    }
}

pub fn query_state(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
) -> StdResult<State> {
    let state=STATE.may_load(deps.storage)?;
    match state {
        Some(val) => Ok(val),
        None => Err(StdError::NotFound {
            kind: String::from("State Not set"),
        }),
    }

}

pub fn query_issued_nft(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    address: String,
) -> StdResult<IssuedNftResponse> {
    let owner = deps.api.addr_validate(&address)?;
    let nft = TOKENS.may_load(deps.storage, owner)?;

    match nft {
        Some(val) => Ok(IssuedNftResponse { nft: val }),
        None => Err(StdError::NotFound {
            kind: String::from("NFT does not exist for the given address"),
        }),
    }
}


pub fn query_issued_vtokens(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    address: Addr,
    denom: String,
) -> StdResult<Vec<Vtoken>> {
    let state=match VTOKENS.may_load(deps.storage,(address,&denom))?
    {
       Some(val)=>val,
       None =>vec![]
    };

    Ok(state)

}


pub fn query_issued_supply(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    denom: String,
) ->StdResult< TokenSupply>{
    let supply= SUPPLY.may_load(deps.storage,&denom)?;
     Ok(supply.unwrap())
}

pub fn query_current_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    app_id: u64,
) ->StdResult<u64>{
    let supply= APPCURRENTPROPOSAL.may_load(deps.storage,app_id)?;
     Ok(supply.unwrap_or_default())
}

pub fn query_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    proposal_id: u64,
) ->StdResult<Proposal>{
    let supply= PROPOSAL.may_load(deps.storage,proposal_id)?;
     Ok(supply.unwrap())
}

pub fn query_bribe(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    app_id:u64,
    proposal_id: u64,
) ->StdResult<Vec<Coin>>{
    let supply= BRIBES_BY_PROPOSAL.may_load(deps.storage,(app_id,proposal_id))?;
     Ok(supply.unwrap())
}

pub fn query_is_voted(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    address:Addr,
    proposal_id: u64,
) ->StdResult<bool>{
    let supply= VOTERS_VOTE.may_load(deps.storage,(address,proposal_id))?;
     Ok(supply.unwrap())
}

pub fn query_vote(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    address:Addr,
    proposal_id: u64,
) ->StdResult<Vote>{
    let supply= VOTERSPROPOSAL.may_load(deps.storage,(address,proposal_id))?;
     Ok(supply.unwrap())
}


pub fn query_bribe_eligible(
    deps: Deps<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    address:Addr,
    app_id:u64
) ->StdResult<Vec<Coin>>{

    let max_proposal_claimed=MAXPROPOSALCLAIMED.load(deps.storage,(app_id,address)).unwrap_or_default();

    let all_proposals=COMPLETEDPROPOSALS.load(deps.storage,app_id)?;

    let bribe_coins=calculate_bribe_reward(deps,env.clone(),info.clone(),max_proposal_claimed,all_proposals.clone(),app_id);
    Ok(bribe_coins.unwrap_or_default())
}



