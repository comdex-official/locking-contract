use crate::error::ContractError;
use crate::helpers::{query_app_exists, query_extended_pair_by_app, query_pool_by_app};
use crate::state::{
    BRIBES_BY_PROPOSAL, COMPLETEDPROPOSALS, DELEGATED, DELEGATION_INFO, DELEGATION_STATS,
    DELEGATOR_CLAIM, DELEGATOR_CLAIMED_PROPOSALS, PROPOSAL, PROPOSALVOTE, VOTERSPROPOSAL,
};

use comdex_bindings::{ComdexMessages, ComdexQuery};
#[cfg(not(feature = "library"))]
use cosmwasm_std::{
    Addr, BankMsg, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, Response, Uint128,
};
use std::ops::{Div, Mul};

/////
pub fn claim_rewards_delegated(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    delegated_address: Addr,
    proposal_id: Option<u64>,
    app_id: u64,
) -> Result<Response<ComdexMessages>, ContractError> {
    ///// get delegated fees /////
    let delegation_info = DELEGATION_INFO.may_load(deps.storage, delegated_address.clone())?;
    if delegation_info.is_none() {
        return Err(ContractError::CustomError {
            val: "Invalid Delegated Address".to_string(),
        });
    }

    let mut fee_coin = vec![];

    if !info.funds.is_empty() {
        return Err(ContractError::FundsNotAllowed {});
    }
    let all_proposals = match COMPLETEDPROPOSALS.may_load(deps.storage, app_id)? {
        Some(val) => val,
        None => vec![],
    };
    let mut bribe_coins = vec![];

    let delegation_info = delegation_info.unwrap();

    if let Some(..) = proposal_id {
        let delegator_claimed = DELEGATOR_CLAIM
            .load(deps.storage, (info.sender.clone(), proposal_id.unwrap()))
            .unwrap_or_default();
        if delegator_claimed {
            return Err(ContractError::CustomError {
                val: "Already Claimed".to_string(),
            });
        }
        let (user_coin, delegated_coin) = calculate_bribe_reward_proposal_delegated(
            deps.as_ref(),
            env,
            info.clone(),
            proposal_id.unwrap(),
            delegated_address,
        )?;
        bribe_coins = user_coin;
        fee_coin = delegated_coin;

        DELEGATOR_CLAIM.save(
            deps.storage,
            (info.sender.clone(), proposal_id.unwrap()),
            &true,
        )?;
        let mut claimed_proposal =
            match DELEGATOR_CLAIMED_PROPOSALS.may_load(deps.storage, info.sender.clone())? {
                Some(val) => val,
                None => vec![],
            };
        claimed_proposal.push(proposal_id.unwrap());
        claimed_proposal.sort();
        DELEGATOR_CLAIMED_PROPOSALS.save(deps.storage, info.sender.clone(), &claimed_proposal)?;
        bribe_coins.sort_by_key(|element| element.denom.clone());
        fee_coin.sort_by_key(|element| element.denom.clone());
    } else {
        let mut fee_coin: Vec<Coin> = vec![];

        let mut claimed_proposal =
            match DELEGATOR_CLAIMED_PROPOSALS.may_load(deps.storage, info.sender.clone())? {
                Some(val) => val,
                None => vec![],
            };

        for proposal_id in all_proposals {
            let delegator_claimed = DELEGATOR_CLAIM
                .load(deps.storage, (info.sender.clone(), proposal_id))
                .unwrap_or_default();
            if delegator_claimed {
                continue;
            }

            let (user_coin, delegated_coins) = calculate_bribe_reward_proposal_delegated(
                deps.as_ref(),
                env.clone(),
                info.clone(),
                proposal_id,
                delegated_address.clone(),
            )?;
            let mut user_coin = user_coin;
            let mut delegated_coin = delegated_coins;
            bribe_coins.append(&mut user_coin);
            fee_coin.append(&mut delegated_coin);

            DELEGATOR_CLAIM.save(deps.storage, (info.sender.clone(), proposal_id), &true)?;

            claimed_proposal.push(proposal_id);
            claimed_proposal.sort();
        }
        DELEGATOR_CLAIMED_PROPOSALS.save(deps.storage, info.sender.clone(), &claimed_proposal)?;
        bribe_coins.sort_by_key(|element| element.denom.clone());
        fee_coin.sort_by_key(|element| element.denom.clone());
    }
    if !bribe_coins.is_empty() {
        if !fee_coin.is_empty() {
            Ok(Response::new()
                .add_attribute("method", "External Incentive Claimed")
                .add_message(BankMsg::Send {
                    to_address: info.sender.to_string(),
                    amount: bribe_coins,
                })
                .add_message(BankMsg::Send {
                    to_address: info.sender.to_string(),
                    amount: fee_coin,
                }))
        } else {
            Ok(Response::new()
                .add_attribute("method", "External Incentive Claimed")
                .add_message(BankMsg::Send {
                    to_address: info.sender.to_string(),
                    amount: bribe_coins,
                }))
        }
    } else if !fee_coin.is_empty() {
        Ok(Response::new()
            .add_attribute("method", "External Incentive Claimed")
            .add_message(BankMsg::Send {
                to_address: delegation_info.delegated_address.to_string(),
                amount: fee_coin,
            }))
    } else {
        Err(ContractError::CustomError {
            val: String::from("No rewards to claim."),
        })
    }
}

pub fn calculate_bribe_reward_proposal_delegated(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    proposal_id: u64,
    delegated_address: Addr,
) -> Result<(Vec<Coin>, Vec<Coin>), ContractError> {
    let mut bribe_coins: Vec<Coin> = vec![];
    let proposal = PROPOSAL.may_load(deps.storage, proposal_id)?;
    if proposal.is_none() {
        return Err(ContractError::CustomError {
            val: String::from("Proposal does not exist."),
        });
    }
    let proposal = proposal.unwrap();

    let delegation_info = DELEGATION_INFO
        .may_load_at_height(deps.storage, delegated_address.clone(), proposal.height)?
        .unwrap();

    let _vote = VOTERSPROPOSAL.may_load(deps.storage, (delegated_address.clone(), proposal_id))?;

    if let Some(..) = _vote {
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
                let mut _claimable_amount: Uint128 = Uint128::zero();
                if delegation_info
                    .excluded_fee_pair
                    .contains(&pair.extended_pair)
                {
                    _claimable_amount = (Decimal::new(Uint128::from(pair.vote_weight))
                        .div(Decimal::new(Uint128::from(total_vote_weight)))
                        .mul(Decimal::one() - delegation_info.protocol_fees))
                    .mul(coin.amount);
                } else {
                    _claimable_amount = (Decimal::new(Uint128::from(pair.vote_weight))
                        .div(Decimal::new(Uint128::from(total_vote_weight))))
                    .mul(coin.amount);
                }
                let claimable_coin = Coin {
                    amount: _claimable_amount,
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
    let total_bribe_coins = bribe_coins.clone();
    let delegation_user =
        DELEGATED.may_load_at_height(deps.storage, info.sender, proposal.height)?;
    if delegation_user.is_none() {
        return Err(ContractError::CustomError {
            val: String::from("delegation does not exist"),
        });
    }
    let delegation_user = delegation_user.unwrap();
    let delegation = delegation_user
        .delegations
        .into_iter()
        .find(|x| x.delegated_to == delegated_address)
        .unwrap();

    let delegation_stats = DELEGATION_STATS
        .may_load_at_height(deps.storage, delegated_address, proposal.height)?
        .unwrap();

    let mut user_coin: Vec<Coin> = vec![];
    let mut delegated_coin: Vec<Coin> = vec![];

    for coin in total_bribe_coins {
        let amount = coin.amount;
        let mut user_share = amount.mul(Decimal::from_ratio(
            delegation.delegated,
            delegation_stats.total_delegated,
        ));
        let delegated_fee = user_share.mul(delegation_info.delegator_fees);
        user_share -= delegated_fee;
        let user_share_coin = Coin {
            amount: user_share,
            denom: coin.denom.clone(),
        };
        let delegated_share_coin = Coin {
            amount: delegated_fee,
            denom: coin.denom,
        };
        user_coin.push(user_share_coin);
        delegated_coin.push(delegated_share_coin);
    }

    Ok((user_coin, delegated_coin))
}

// update excluded_fee_pair
pub fn update_excluded_fee_pair(
    deps: DepsMut<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    delegation_address: Addr,
    harbor_app_id: u64,
    cswap_app_id: u64,
    excluded_fee_pair: Vec<u64>,
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
    //check if app exist
    let _ = query_app_exists(deps.as_ref(), harbor_app_id)?;

    //check if app exist
    let _ = query_app_exists(deps.as_ref(), cswap_app_id)?;

    //get ext pairs vec from app
    let ext_pairs = query_extended_pair_by_app(deps.as_ref(), harbor_app_id)?;

    //get pools vec from app
    let mut pools = query_pool_by_app(deps.as_ref(), cswap_app_id)?;
    for pool in pools.iter_mut() {
        *pool *= 1000000;
    }

    for pair in excluded_fee_pair.clone() {
        if delegation_info.excluded_fee_pair.contains(&pair)
            || !ext_pairs.contains(&pair)
            || !pools.contains(&pair)
        {
            continue;
        }
        delegation_info.excluded_fee_pair.push(pair);
    }
    DELEGATION_INFO.save(
        deps.storage,
        delegation_address,
        &delegation_info,
        env.block.height,
    )?;
    Ok(Response::new()
        .add_attribute("method", "update_excluded_fee_pair")
        .add_attribute(
            "excluded_fee_pair",
            format!("{:?}", delegation_info.excluded_fee_pair),
        ))
}
