// !------- IssuedVtokens query not implemented-------!

use comdex_bindings::ComdexQuery;
use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Coin, Deps, Env, MessageInfo, StdError, StdResult,
    Uint128,
};

use crate::contract::calculate_bribe_reward;
use crate::msg::{IssuedNftResponse, IssuedVtokensResponse, QueryMsg, WithdrawableResponse};
use crate::state::{
    Proposal, State, TokenSupply, Vote, Vtoken, APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL,
    COMPLETEDPROPOSALS, MAXPROPOSALCLAIMED, PROPOSAL, STATE, SUPPLY, TOKENS, VOTERSPROPOSAL,
    VOTERS_VOTE, VTOKENS,
};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(
    deps: Deps<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::IssuedNft { address } => to_binary(&query_issued_nft(deps, env, info, address)?),

        QueryMsg::IssuedVtokens { address, denom } => {
            to_binary(&query_issued_vtokens(deps, env, info, address, denom)?)
        }

        QueryMsg::Supply { denom } => to_binary(&query_issued_supply(deps, env, info, denom)?),

        QueryMsg::CurrentProposal { app_id } => {
            to_binary(&query_current_proposal(deps, env, info, app_id)?)
        }

        QueryMsg::Proposal { proposal_id } => {
            to_binary(&query_proposal(deps, env, info, proposal_id)?)
        }

        QueryMsg::BribeByProposal {
            proposal_id,
            app_id,
        } => to_binary(&query_bribe(deps, env, info, app_id, proposal_id)?),

        QueryMsg::Vote {
            proposal_id,
            address,
        } => to_binary(&query_vote(deps, env, info, address, proposal_id)?),

        QueryMsg::ClaimableBribe { address, app_id } => {
            to_binary(&query_bribe_eligible(deps, env, info, address, app_id)?)
        }

        QueryMsg::Withdrawable { address, denom } => {
            to_binary(&query_withdrawable(deps, env, info, address, denom)?)
        }

        _ => panic!("Not implemented"),
    }
}

pub fn query_state(deps: Deps<ComdexQuery>, _env: Env, _info: MessageInfo) -> StdResult<State> {
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
    let state = match VTOKENS.may_load(deps.storage, (address, &denom))? {
        Some(val) => val,
        None => vec![],
    };

    Ok(state)
}

pub fn query_issued_supply(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    denom: String,
) -> StdResult<TokenSupply> {
    let supply = SUPPLY.may_load(deps.storage, &denom)?;
    Ok(supply.unwrap())
}

pub fn query_current_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    app_id: u64,
) -> StdResult<u64> {
    let supply = APPCURRENTPROPOSAL.may_load(deps.storage, app_id)?;
    Ok(supply.unwrap_or_default())
}

pub fn query_proposal(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    proposal_id: u64,
) -> StdResult<Proposal> {
    let supply = PROPOSAL.may_load(deps.storage, proposal_id)?;
    Ok(supply.unwrap())
}

pub fn query_bribe(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    app_id: u64,
    proposal_id: u64,
) -> StdResult<Vec<Coin>> {
    let supply = BRIBES_BY_PROPOSAL.may_load(deps.storage, (app_id, proposal_id))?;
    Ok(supply.unwrap())
}

pub fn query_is_voted(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    address: Addr,
    proposal_id: u64,
) -> StdResult<bool> {
    let supply = VOTERS_VOTE.may_load(deps.storage, (address, proposal_id))?;
    Ok(supply.unwrap())
}

pub fn query_vote(
    deps: Deps<ComdexQuery>,
    _env: Env,
    _info: MessageInfo,
    address: Addr,
    proposal_id: u64,
) -> StdResult<Vote> {
    let supply = VOTERSPROPOSAL.may_load(deps.storage, (address, proposal_id))?;
    Ok(supply.unwrap())
}

pub fn query_withdrawable(
    deps: Deps<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    address: Option<String>,
    denom: String,
) -> StdResult<WithdrawableResponse> {
    let address = if let Some(val) = address {
        deps.api.addr_validate(&val)?
    } else {
        info.sender
    };

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
    info: MessageInfo,
    address: Addr,
    app_id: u64,
) -> StdResult<Vec<Coin>> {
    let max_proposal_claimed = MAXPROPOSALCLAIMED
        .load(deps.storage, (app_id, address))
        .unwrap_or_default();

    let all_proposals = COMPLETEDPROPOSALS.load(deps.storage, app_id)?;

    let bribe_coins = calculate_bribe_reward(
        deps,
        env.clone(),
        info.clone(),
        max_proposal_claimed,
        all_proposals.clone(),
        app_id,
    );
    Ok(bribe_coins.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{execute, instantiate};
    use crate::msg::{ExecuteMsg, InstantiateMsg};
    use crate::state::{LockingPeriod, PeriodWeight, Status};
    use comdex_bindings::ComdexQuery;
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{coins, Decimal, OwnedDeps, Timestamp, Uint128};
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
        let mut env = mock_env();
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
        VTOKENS.save(deps.as_mut().storage, (info.sender.clone(), DENOM), &data);

        // Query the withdrawable balance; should be 250
        let res = query_withdrawable(
            deps.as_ref(),
            env.clone(),
            info.clone(),
            None,
            DENOM.to_string(),
        )
        .unwrap();
        assert_eq!(
            res.amount,
            Coin {
                denom: DENOM.to_string(),
                amount: Uint128::from(250u128)
            }
        );
    }
}
