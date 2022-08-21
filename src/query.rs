// !------- IssuedVtokens query not implemented-------!

use std::borrow::Borrow;

use comdex_bindings::ComdexQuery;
use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Coin, Deps, Env, StdError, StdResult,
    Uint128
};
use crate::error::ContractError;
use cw_storage_plus::Bound;
use std::cmp::Ordering;
use crate::msg::{IssuedNftResponse, QueryMsg, WithdrawableResponse};
use crate::state::{
    Proposal, TokenSupply, Vote, Vtoken, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL,
    COMPLETEDPROPOSALS, MAXPROPOSALCLAIMED, PROPOSAL, SUPPLY, TOKENS, VOTERSPROPOSAL,
    VOTERS_VOTE, VTOKENS,PROPOSALVOTE,State,STATE,EMISSION,Emission
};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<ComdexQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::IssuedNft { address } => to_binary(&query_issued_nft(deps, env, address)?),
        QueryMsg::IssuedVtokens { address, denom } => {to_binary(&query_issued_vtokens(deps, env, address, denom)?)},
        QueryMsg::Supply { denom } => to_binary(&query_issued_supply(deps, env, denom)?),
        QueryMsg::CurrentProposal { app_id } => {to_binary(&query_current_proposal(deps, env, app_id)?)},
        QueryMsg::Proposal { proposal_id } => to_binary(&query_proposal(deps, env, proposal_id)?),
        QueryMsg::BribeByProposal {proposal_id,extended_pair_id,} => to_binary(&query_bribe(deps, env, proposal_id, extended_pair_id)?),
        QueryMsg::Vote {proposal_id,address} => to_binary(&query_vote(deps, env, address, proposal_id)?),
        QueryMsg::ClaimableBribe { address, app_id } => {to_binary(&query_bribe_eligible(deps, env, address, app_id)?)},
        QueryMsg::Withdrawable { address, denom } => {to_binary(&query_withdrawable(deps, env, address, denom)?)},
        QueryMsg::TotalVTokens{address,denom} =>  {to_binary(&query_vtoken_balance(deps, env,address,denom)?)},
        QueryMsg::State {  } => to_binary(&query_state(deps, env)?),
        QueryMsg::Emission { app_id } => to_binary(&query_emission(deps, env, app_id)?),
        QueryMsg::ExtendedPairVote { proposal_id, extended_pair_id } => to_binary(&query_extendedpairvote(deps, env, proposal_id,extended_pair_id)?),
        QueryMsg::HasVoted { address, proposal_id } =>to_binary(&query_is_voted(deps, env, address,proposal_id)?),
        _ => panic!("Not implemented"),
    }
}



pub fn query_emission(deps: Deps<ComdexQuery>, _env: Env, proposal_id: u64) -> StdResult<Option<Emission>> {
    let supply = EMISSION.may_load(deps.storage, proposal_id)?;
    Ok(supply)
}

pub fn query_extendedpairvote(deps: Deps<ComdexQuery>, _env: Env,proposal_id:u64, extended_pair_id: u64) -> StdResult<Option<Uint128>> {
    let supply = PROPOSALVOTE.may_load(deps.storage, (proposal_id,extended_pair_id))?;
    
    Ok(supply)
}


pub fn query_vtoken_balance(deps: Deps<ComdexQuery>, _env: Env,address: Addr,
    denom:String) -> StdResult<Uint128> {
    let vtokens = VTOKENS.may_load(deps.storage, (address, &denom))?;
    if vtokens.is_none()
    {
        return Ok(Uint128::zero());
    }

    let  vtokens=vtokens.unwrap();
    let mut total_vtoken:u128=0;
    for vtoken in vtokens.clone() {
        total_vtoken += vtoken.vtoken.amount.u128();
    }    
    
    Ok(Uint128::from(total_vtoken))
}

pub fn query_state(deps: Deps<ComdexQuery>, _env: Env) -> StdResult<State> {
    let state = STATE.may_load(deps.storage)?;
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
    address: Addr,
    denom: String,
) -> StdResult<Vec<Vtoken>> {
    let state = match VTOKENS.may_load(deps.storage, (address, &denom))? {
        Some(val) => val,
        None => vec![],
    };

    Ok(state)
}


pub fn query_issued_supply(
    deps: Deps<ComdexQuery>,
    _env: Env,
    denom: String,
) -> StdResult<Option<TokenSupply>> {
    let supply = SUPPLY.may_load(deps.storage, &denom)?;
    Ok(supply)
}

pub fn query_current_proposal(deps: Deps<ComdexQuery>, _env: Env, app_id: u64) -> StdResult<u64> {
    let supply = APPCURRENTPROPOSAL.may_load(deps.storage, app_id)?;
    Ok(supply.unwrap_or(0))
}

pub fn query_proposal(deps: Deps<ComdexQuery>, _env: Env, proposal_id: u64) -> StdResult<Option<Proposal>> {
    let supply = PROPOSAL.may_load(deps.storage, proposal_id)?;
    Ok(supply)
}

pub fn query_bribe(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
    extended_pair_id:u64,
) -> StdResult<Option<Vec<Coin>>> {
    let supply = BRIBES_BY_PROPOSAL.may_load(deps.storage, (proposal_id, extended_pair_id))?;
    Ok(supply)
}

pub fn query_is_voted(
    deps: Deps<ComdexQuery>,
    _env: Env,
    address: Addr,
    proposal_id: u64,
) -> StdResult<bool> {
    let supply = VOTERS_VOTE.may_load(deps.storage, (address, proposal_id))?;
    Ok(supply.unwrap_or(false))
}

pub fn query_vote(
    deps: Deps<ComdexQuery>,
    _env: Env,
    address: Addr,
    proposal_id: u64,
) -> StdResult<Option<Vote>> {
    let supply = VOTERSPROPOSAL.may_load(deps.storage, (address, proposal_id))?;
    Ok(supply)
}

pub fn query_withdrawable(
    deps: Deps<ComdexQuery>,
    env: Env,
    address: String,
    denom: String,
) -> StdResult<WithdrawableResponse> {
    let address = deps.api.addr_validate(&address)?;

    let vtokens = VTOKENS.may_load(deps.storage, (address.clone(), &denom))?;

    if let None = vtokens {
        return Err(StdError::NotFound {
            kind: format!("No token found for {:?}", denom),
        });
    }

    let vtokens = vtokens.unwrap();

    let withdraw_amount: u128 = vtokens
        .into_iter()
        .filter(|el| el.token.denom == denom && el.end_time < env.block.time)
        .fold(0u128, |acc, el| acc + el.token.amount.u128());

    Ok(WithdrawableResponse {
        amount: Coin {
            denom: denom,
            amount: Uint128::from(withdraw_amount),
        },
    })
}

pub fn query_bribe_eligible(
    deps: Deps<ComdexQuery>,
    env: Env,
    address: Addr,
    app_id: u64,
) -> StdResult<Vec<Coin>> {
    let max_proposal_claimed = MAXPROPOSALCLAIMED
        .load(deps.storage, (app_id, address.clone()))
        .unwrap_or_default();

    let all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)?
    {
        Some(val)=>val,
        None => vec![]
    };


    let bribe_coins = calculate_bribe_reward_query(
        deps,
        env.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        address.borrow(),
        app_id,
    );
    Ok(bribe_coins.unwrap_or_default())
}


pub fn calculate_bribe_reward_query(
    deps: Deps<ComdexQuery>,
    _env: Env,
    max_proposal_claimed: u64,
    all_proposals: Vec<u64>,
    address: &Addr,
    _app_id: u64,
) -> Result<Vec<Coin>, ContractError> {
    //check if active proposal
    let mut bribe_coins: Vec<Coin> = vec![];
    for proposalid in all_proposals.clone() {
        if proposalid <= max_proposal_claimed {
            continue;
        }
        let vote = match VOTERSPROPOSAL.may_load(deps.storage, (address.to_owned(), proposalid))?
         {
            Some(val) => val,
            None => continue
        };
        let proposal1 = match PROPOSAL.may_load(deps.storage, proposalid)?
        {
            Some(val) => val,
            None => continue
        };

        let total_vote_weight = PROPOSALVOTE
            .load(deps.storage, (proposal1.app_id, vote.extended_pair))?
            .u128();
        let total_bribe =match BRIBES_BY_PROPOSAL.may_load(deps.storage, (proposal1.app_id, vote.extended_pair))?
        {
            Some(val) => val,
            None => vec![],
        };

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
    Ok(bribe_coins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{LockingPeriod,  Status};
    use comdex_bindings::ComdexQuery;
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{OwnedDeps, Timestamp, Uint128};
    use std::marker::PhantomData;

    const DENOM: &str = "TKN";

    fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, ComdexQuery> {
        OwnedDeps {
            storage: MockStorage::default(),
            api: MockApi::default(),
            querier: MockQuerier::default(),
            custom_query_type: PhantomData,
        }
    }

    #[test]
    fn withdrawable() {
        let mut deps = mock_dependencies();
        let  env = mock_env();
        let info = mock_info("sender", &[]);

        // Store some test vtokens
        let data = vec![
            Vtoken {
                token: Coin {
                    denom: DENOM.to_string(),
                    amount: Uint128::from(1000u128),
                },
                vtoken: Coin {
                    denom: "vTKN".to_string(),
                    amount: Uint128::zero(),
                },
                period: LockingPeriod::T1,
                start_time: env.block.time,
                end_time: env.block.time.plus_seconds(100_000),
                status: Status::Locked,
            },
            Vtoken {
                token: Coin {
                    denom: DENOM.to_string(),
                    amount: Uint128::from(250u128),
                },
                vtoken: Coin {
                    denom: "vTKN".to_string(),
                    amount: Uint128::zero(),
                },
                period: LockingPeriod::T1,
                start_time: Timestamp::from_seconds(0),
                end_time: Timestamp::from_seconds(20),
                status: Status::Locked,
            },
        ];
        _=VTOKENS.save(deps.as_mut().storage, (info.sender.clone(), DENOM), &data);

        // Query the withdrawable balance; should be 250
        // let res = query_withdrawable(deps.as_ref(), env.clone(), DENOM.to_string())
        //     .unwrap();
        // assert_eq!(
        //     res.amount,
        //     Coin {
        //         denom: DENOM.to_string(),
        //         amount: Uint128::from(250u128)
        //     }
        // );
    }
}
