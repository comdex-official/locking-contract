use crate::error::ContractError;
use crate::helpers::get_token_supply;
use crate::msg::{IssuedNftResponse, QueryMsg, WithdrawableResponse};
use crate::state::{
    Delegation, DelegationInfo, DelegationStats, Emission, EmissionVaultPool, LockingPeriod,
    Proposal, RebaseAllResponse, RewardAllResponse, State, TokenSupply, UserDelegationInfo, Vote,
    VoteResponse, Vtoken, ADMIN, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, COMPLETEDPROPOSALS,
    DELEGATED, DELEGATION_INFO, DELEGATION_STATS, EMISSION, EMISSION_REWARD, PROPOSAL,
    PROPOSALVOTE, REBASE_CLAIMED, STATE, SUPPLY, TOKENS, VOTERSPROPOSAL, VOTERS_CLAIM, VOTERS_VOTE,
    VTOKENS,
};
use comdex_bindings::{ComdexQuery, GetPoolByAppResponse};
use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Coin, Decimal, Deps, Env, QueryRequest, StdError,
    StdResult, Uint128, WasmQuery,
};
use std::ops::{Div, Mul};
const MAX_LIMIT: u32 = 30;
const DEFAULT_LIMIT: u32 = 10;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps<ComdexQuery>, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::IssuedNft { address } => to_binary(&query_issued_nft(deps, env, address)?),
        QueryMsg::IssuedVtokens {
            address,
            denom,
            start_after,
            limit,
        } => to_binary(&query_issued_vtokens(
            deps,
            env,
            address,
            denom,
            start_after,
            limit,
        )?),
        QueryMsg::Supply { denom } => to_binary(&query_issued_supply(deps, env, denom)?),
        QueryMsg::CurrentProposal { app_id } => {
            to_binary(&query_current_proposal(deps, env, app_id)?)
        }
        QueryMsg::Proposal { proposal_id } => to_binary(&query_proposal(deps, env, proposal_id)?),
        QueryMsg::BribeByProposal {
            proposal_id,
            extended_pair_id,
        } => to_binary(&query_bribe(deps, env, proposal_id, extended_pair_id)?),
        QueryMsg::Vote {
            proposal_id,
            address,
        } => to_binary(&query_vote(deps, env, address, proposal_id)?),
        QueryMsg::ClaimableBribe { address, app_id } => {
            to_binary(&query_bribe_eligible(deps, env, address, app_id)?)
        }
        QueryMsg::Withdrawable { address, denom } => {
            to_binary(&query_withdrawable(deps, env, address, denom)?)
        }
        QueryMsg::TotalVTokens {
            address,
            denom,
            height,
        } => to_binary(&query_vtoken_balance(deps, env, address, denom, height)?),
        QueryMsg::State {} => to_binary(&query_state(deps, env)?),
        QueryMsg::Emission { app_id } => to_binary(&query_emission(deps, env, app_id)?),
        QueryMsg::ExtendedPairVote {
            proposal_id,
            extended_pair_id,
        } => to_binary(&query_extendedpairvote(
            deps,
            env,
            proposal_id,
            extended_pair_id,
        )?),
        QueryMsg::HasVoted {
            address,
            proposal_id,
        } => to_binary(&query_is_voted(deps, env, address, proposal_id)?),
        QueryMsg::UserProposalAllUp {
            proposal_id,
            address,
        } => to_binary(&query_proposal_all_up(deps, env, address, proposal_id)?),
        QueryMsg::Rebase {
            app_id,
            address,
            denom,
        } => to_binary(&query_rebase_eligible(deps, env, address, app_id, denom)?),
        QueryMsg::Admin {} => to_binary(&query_admin(deps, env)?),
        QueryMsg::EmissionRewards { proposal_id } => {
            to_binary(&query_proposal_rewards(deps, env, proposal_id)?)
        }
        QueryMsg::ProjectedEmission {
            proposal_id,
            app_id,
            gov_token_denom,
            gov_token_id,
        } => to_binary(&query_emission_proposal(
            deps,
            env,
            proposal_id,
            app_id,
            gov_token_denom,
            gov_token_id,
        )?),
        QueryMsg::CurrentProposalUser { app_id, address } => {
            to_binary(&query_current_proposal_user(deps, env, address, app_id)?)
        }
        QueryMsg::DelegationRequest {
            delegated_address,
            delegator_address,
            height,
        } => to_binary(&query_delegation(
            deps,
            env,
            delegated_address,
            delegator_address,
            height,
        )?),
        QueryMsg::DelegatorParamRequest { delegated_address } => {
            to_binary(&query_delegator_param(deps, env, delegated_address)?)
        }
        QueryMsg::DelegationStats {
            delegated_address,
            height,
        } => to_binary(&query_delegated_stats(
            deps,
            env,
            delegated_address,
            height,
        )?),
        QueryMsg::UserDelegationStats {
            delegator_address,
            height,
        } => to_binary(&query_user_delegation_all(
            deps,
            env,
            delegator_address,
            height,
        )?),
        QueryMsg::UserEmissionVoting {
            address,
            proposal_id,
            denom,
        } => to_binary(&query_emission_voting_power(
            deps,
            env,
            address,
            proposal_id,
            denom,
        )?),
        _ => panic!("Not implemented"),
    }
}

pub fn query_pool_by_app(
    deps: Deps<ComdexQuery>,
    app_mapping_id_param: u64,
) -> StdResult<Vec<u64>> {
    let pool_pair = deps
        .querier
        .query::<GetPoolByAppResponse>(&QueryRequest::Custom(ComdexQuery::GetPoolByApp {
            app_id: app_mapping_id_param,
        }))?;

    Ok(pool_pair.pools)
}

pub fn query_admin(deps: Deps<ComdexQuery>, _env: Env) -> StdResult<Option<Addr>> {
    let admin = ADMIN.get(deps)?;
    Ok(admin)
}

pub fn query_emission(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
) -> StdResult<Option<Emission>> {
    let supply = EMISSION.may_load(deps.storage, proposal_id)?;
    Ok(supply)
}

pub fn query_emission_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
    app_id: u64,
    gov_token_denom: String,
    gov_token_id: u64,
) -> StdResult<u128> {
    let proposal = PROPOSAL.load(deps.storage, proposal_id)?;

    let vtokens = SUPPLY
        .may_load_at_height(deps.storage, &gov_token_denom, proposal.height)?
        .unwrap();

    let total_v_token = vtokens.vtoken;
    let total_weight = get_token_supply(deps, app_id, gov_token_id)?;
    let state = STATE.load(deps.storage)?;
    let query_msg = QueryMsg::VestedTokens {
        denom: gov_token_denom,
    };
    let query_response: Uint128 = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: state.vesting_contract.to_string(),
        msg: to_binary(&query_msg).unwrap(),
    }))?;
    let circulating_supply =
        Uint128::from(total_weight) - query_response - Uint128::from(vtokens.token);

    let percentage_locked =
        Decimal::raw(total_v_token).div(Decimal::raw(circulating_supply.u128() + total_v_token));
    let emission = EMISSION.load(deps.storage, proposal.app_id)?;
    let reward_emission = Uint128::from(emission.rewards_pending) * emission.emission_rate;
    let effective_emission = reward_emission.mul(Decimal::one() - percentage_locked);
    let emission_distributed =
        effective_emission.u128() - (state.foundation_percentage.mul(effective_emission)).u128();

    Ok(emission_distributed)
}

pub fn query_extendedpairvote(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
    extended_pair_id: u64,
) -> StdResult<Option<Uint128>> {
    let supply = PROPOSALVOTE.may_load(deps.storage, (proposal_id, extended_pair_id))?;

    Ok(supply)
}

pub fn query_vtoken_balance(
    deps: Deps<ComdexQuery>,
    env: Env,
    address: Addr,
    denom: String,
    height: Option<u64>,
) -> StdResult<Uint128> {
    deps.api.addr_validate(&address.clone().into_string())?;
    let query_height = height.unwrap_or(env.block.height);
    let vtokens = VTOKENS.may_load_at_height(deps.storage, (address, &denom), query_height)?;
    if vtokens.is_none() {
        return Ok(Uint128::zero());
    }

    let vtokens = vtokens.unwrap();
    let mut total_vtoken: u128 = 0;
    for vtoken in vtokens {
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
    start_after: u32,
    limit: Option<u32>,
) -> StdResult<Vec<Vtoken>> {
    deps.api.addr_validate(&address.clone().into_string())?;

    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after as usize;
    let checkpoint = start + limit;
    let state = match VTOKENS.may_load(deps.storage, (address, &denom))? {
        Some(val) => {
            // If the vec len is smaller than start_after, then empty vec
            // If the checkpoint is >= length, then return all remaining elements
            // else return the specific elements
            let length = val.len();
            if length <= start {
                vec![]
            } else if checkpoint >= length {
                val[start..].to_vec()
            } else {
                val[start..checkpoint].to_vec()
            }
        }
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

pub fn query_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
) -> StdResult<Option<Proposal>> {
    let supply = PROPOSAL.may_load(deps.storage, proposal_id)?;
    Ok(supply)
}

pub fn query_proposal_rewards(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
) -> StdResult<Option<EmissionVaultPool>> {
    let supply = EMISSION_REWARD.may_load(deps.storage, proposal_id)?;
    Ok(supply)
}

pub fn query_bribe(
    deps: Deps<ComdexQuery>,
    _env: Env,
    proposal_id: u64,
    extended_pair_id: u64,
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

pub fn query_proposal_all_up(
    deps: Deps<ComdexQuery>,
    _env: Env,
    address: Addr,
    proposal_id: u64,
) -> StdResult<Vote> {
    deps.api.addr_validate(&address.clone().into_string())?;

    let vote = VOTERSPROPOSAL
        .may_load(deps.storage, (address, proposal_id))?
        .unwrap();
    Ok(vote)
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

    let vtokens = VTOKENS.may_load(deps.storage, (address, &denom))?;

    if vtokens.is_none() {
        return Err(StdError::NotFound {
            kind: format!("No token found for {:?}", denom),
        });
    }

    let vtokens = vtokens.unwrap();
    let denom_param = denom.to_owned();
    let withdraw_amount: u128 = vtokens
        .into_iter()
        .filter(|el| el.token.denom == denom && el.end_time < env.block.time)
        .fold(0u128, |acc, el| acc + el.token.amount.u128());

    Ok(WithdrawableResponse {
        amount: Coin {
            denom: denom_param,
            amount: Uint128::from(withdraw_amount),
        },
    })
}

pub fn query_bribe_eligible(
    deps: Deps<ComdexQuery>,
    env: Env,
    address: Addr,
    app_id: u64,
) -> StdResult<Vec<RewardAllResponse>> {
    let all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };

    let bribe_coins =
        calculate_bribe_reward_query(deps, env, all_proposals, address.clone(), app_id).unwrap();
    Ok(bribe_coins)
}

pub fn calculate_bribe_reward_query(
    deps: Deps<ComdexQuery>,
    _env: Env,
    all_proposals: Vec<u64>,
    address: Addr,
    _app_id: u64,
) -> Result<Vec<RewardAllResponse>, ContractError> {
    let mut resp: Vec<RewardAllResponse> = vec![];
    //check if active proposal
    for proposalid in all_proposals {
        let mut bribe_coins: Vec<Coin> = vec![];
        let claimed = VOTERS_CLAIM
            .may_load(deps.storage, (address.clone(), proposalid))?
            .unwrap_or_default();
        let vote = match VOTERSPROPOSAL.may_load(deps.storage, (address.to_owned(), proposalid))? {
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
        let response = RewardAllResponse {
            proposal_id: proposalid,
            total_incentive: bribe_coins,
            claimed: claimed,
        };
        resp.push(response);
    }

    //// send bank message to band

    Ok(resp)
}

pub fn query_rebase_eligible(
    deps: Deps<ComdexQuery>,
    _env: Env,
    address: Addr,
    app_id: u64,
    denom: String,
) -> StdResult<Vec<RebaseAllResponse>> {
    let mut response: Vec<RebaseAllResponse> = vec![];
    let all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };
    for proposal_id_param in all_proposals {
        let has_rebased = REBASE_CLAIMED
            .may_load(deps.storage, (address.clone(), proposal_id_param))?
            .unwrap_or_default();
        let proposal = PROPOSAL.load(deps.storage, proposal_id_param)?;
        if !proposal.emission_completed {
            continue;
        } else {
            let supply = SUPPLY
                .may_load_at_height(deps.storage, &denom, proposal.height)?
                .unwrap();

            let total_locked: u128 = supply.token;
            let total_rebase_amount: u128 = proposal.rebase_distributed;
            let vtokens = match VTOKENS.may_load_at_height(
                deps.storage,
                (address.clone(), &denom),
                proposal.height,
            )? {
                Some(val) => val,
                None => vec![],
            };
            if vtokens.is_empty() {
                continue;
            }
            let mut locked_t1: u128 = 0;
            let mut locked_t2: u128 = 0;

            for vtoken in vtokens {
                match vtoken.period {
                    LockingPeriod::T1 => locked_t1 += vtoken.token.amount.u128(),
                    LockingPeriod::T2 => locked_t2 += vtoken.token.amount.u128(),
                }
            }
            let sum = locked_t1 + locked_t2;
            let rebase_amount_param = (Uint128::from(total_rebase_amount)
                .checked_mul(Uint128::from(sum))?)
            .checked_div(Uint128::from(total_locked))?;
            let rebase_response = RebaseAllResponse {
                proposal_id: proposal_id_param,
                rebase: rebase_amount_param,
                claimed: has_rebased,
            };
            response.push(rebase_response);
        }
    }

    Ok(response)
}

pub fn query_current_proposal_user(
    deps: Deps<ComdexQuery>,
    _env: Env,
    address: Addr,
    app_id: u64,
) -> StdResult<Vec<VoteResponse>> {
    let current_proposal = APPCURRENTPROPOSAL.may_load(deps.storage, app_id)?;

    let current_proposal = current_proposal.unwrap();
    let proposal = PROPOSAL.load(deps.storage, current_proposal)?;

    let mut resp = vec![];
    for ext_pair in proposal.extended_pair {
        let mut user_vote = 0;
        let mut user_vote_ratio = Decimal::zero();
        let mut total_incentive = vec![];
        let mut total_vote = 0;
        let proposal_vote = PROPOSALVOTE.may_load(deps.storage, (current_proposal, ext_pair))?;
        if let Some(..) = proposal_vote {
            let proposal_vote = proposal_vote.unwrap();
            total_vote = proposal_vote.u128();
        }
        let vote = VOTERSPROPOSAL.may_load(deps.storage, (address.clone(), current_proposal))?;

        if let Some(..) = vote {
            let vote = vote.unwrap();
            let vote = vote.votes;
            let vote_tmp = vote.into_iter().find(|x| x.extended_pair == ext_pair);
            if let Some(..) = vote_tmp {
                let vote_tmp = vote_tmp.unwrap();
                user_vote = vote_tmp.vote_weight;
                user_vote_ratio = vote_tmp.vote_ratio;
            }
        }
        //// load bribe////
        let bribe = BRIBES_BY_PROPOSAL.may_load(deps.storage, (current_proposal, ext_pair))?;
        if let Some(..) = bribe {
            let bribe = bribe.unwrap();
            total_incentive = bribe;
        }
        let vote_response = VoteResponse {
            pair: ext_pair,
            user_vote: user_vote,
            user_vote_ratio: user_vote_ratio,
            total_incentive: total_incentive,
            total_vote: total_vote,
        };
        resp.push(vote_response);
    }
    Ok(resp)
}

pub fn query_delegation(
    deps: Deps<ComdexQuery>,
    _env: Env,
    delegated_address: Addr,
    delegator_address: Addr,
    height: Option<u64>,
) -> StdResult<Option<Delegation>> {
    if height.is_some() {
        let delegation_user =
            DELEGATED.may_load_at_height(deps.storage, delegator_address, height.unwrap())?;
        if delegation_user.is_none() {
            return Ok(None);
        }
        let delegation_user = delegation_user.unwrap();
        let delegation = delegation_user
            .delegations
            .into_iter()
            .find(|x| x.delegated_to == delegated_address);
        Ok(delegation)
    } else {
        let delegation_user = DELEGATED.may_load(deps.storage, delegator_address)?;
        if delegation_user.is_none() {
            return Ok(None);
        }
        let delegation_user = delegation_user.unwrap();
        let delegation = delegation_user
            .delegations
            .into_iter()
            .find(|x| x.delegated_to == delegated_address);
        Ok(delegation)
    }
}
pub fn query_delegator_param(
    deps: Deps<ComdexQuery>,
    _env: Env,
    delegated_address: Addr,
) -> StdResult<Option<DelegationInfo>> {
    let delegation_info = DELEGATION_INFO.may_load(deps.storage, delegated_address)?;
    Ok(delegation_info)
}

pub fn query_delegated_stats(
    deps: Deps<ComdexQuery>,
    _env: Env,
    delegated_address: Addr,
    height: Option<u64>,
) -> StdResult<Option<DelegationStats>> {
    if height.is_some() {
        let delegation_stats = DELEGATION_STATS.may_load_at_height(
            deps.storage,
            delegated_address,
            height.unwrap(),
        )?;
        return Ok(delegation_stats);
    } else {
        let delegation_stats = DELEGATION_STATS.may_load(deps.storage, delegated_address)?;
        return Ok(delegation_stats);
    }
}

pub fn query_user_delegation_all(
    deps: Deps<ComdexQuery>,
    _env: Env,
    delegator_address: Addr,
    height: Option<u64>,
) -> StdResult<Option<UserDelegationInfo>> {
    if height.is_some() {
        let user_delegation_info =
            DELEGATED.may_load_at_height(deps.storage, delegator_address, height.unwrap())?;
        return Ok(user_delegation_info);
    } else {
        let user_delegation_info = DELEGATED.may_load(deps.storage, delegator_address)?;
        return Ok(user_delegation_info);
    }
}

pub fn query_emission_voting_power(
    deps: Deps<ComdexQuery>,
    _env: Env,
    address: Addr,
    proposal_id: u64,
    denom: String,
) -> StdResult<u128> {
    let proposal = PROPOSAL.may_load(deps.storage, proposal_id)?.unwrap();
    let vtokens =
        VTOKENS.may_load_at_height(deps.storage, (address.clone(), &denom), proposal.height)?;
    if vtokens.is_none() {
        return Ok(0);
    }
    let mut vote_power: u128 = 0;
    for vtoken in vtokens.unwrap() {
        vote_power += vtoken.token.amount.u128();
    }

    let delegation = DELEGATED.may_load_at_height(deps.storage, address, proposal.height)?;
    if let Some(..) = delegation {
        let delegation = delegation.unwrap();
        vote_power -= delegation.total_casted;
    }
    Ok(vote_power)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{LockingPeriod, Status};
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
        let env = mock_env();
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
        _ = VTOKENS.save(
            deps.as_mut().storage,
            (info.sender.clone(), DENOM),
            &data,
            env.block.height,
        );
    }
}
