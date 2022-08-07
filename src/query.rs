// !------- IssuedVtokens query not implemented-------!

use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Coin, Deps, Env, MessageInfo, StdError, StdResult,
};

use crate::msg::{
    IssuedNftResponse, IssuedVtokensResponse, LockedTokensResponse, QueryMsg,
    UnlockedTokensResponse, UnlockingTokensResponse,
};
use crate::state::{Status, LOCKED, TOKENS, UNLOCKED, UNLOCKING, VTOKENS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, info: MessageInfo, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::IssuedNft { address } => to_binary(&query_issued_nft(deps, env, info, address)?),

        QueryMsg::UnlockedTokens { address, denom } => {
            to_binary(&query_unlocked_tokens(deps, env, info, address, denom)?)
        }

        QueryMsg::UnlockingTokens { address, denom } => {
            to_binary(&query_unlocking_tokens(deps, env, info, address, denom)?)
        }

        QueryMsg::LockedTokens { address, denom } => {
            to_binary(&query_locked_tokens(deps, env, info, address, denom)?)
        }

        QueryMsg::IssuedVtokens { address } => {
            to_binary(&query_issued_vtokens(deps, env, info, address)?)
        }
    }
}

pub fn query_issued_nft(
    deps: Deps,
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

pub fn query_unlocked_tokens(
    deps: Deps,
    _env: Env,
    info: MessageInfo,
    address: Option<String>,
    denom: Option<String>,
) -> StdResult<UnlockedTokensResponse> {
    // set `owner` for querying tokens
    let owner: Addr;
    if let Some(val) = address {
        owner = deps.api.addr_validate(&val)?;
    } else {
        owner = info.sender;
    };

    // result contains either a single token for the given denom or all
    // unlocked tokens for the given owner
    let vtokens: Vec<Coin>;
    if let Some(denom) = denom {
        let res = VTOKENS.load(deps.storage, (owner, &denom))?;
        if res.status != Status::Unlocked {
            return Err(StdError::GenericErr {
                msg: String::from("No unlocked tokens for given denom"),
            });
        }
        vtokens = vec![res.vtoken];
    } else {
        vtokens = UNLOCKED.load(deps.storage, owner)?;
    }

    Ok(UnlockedTokensResponse { tokens: vtokens })
}

pub fn query_locked_tokens(
    deps: Deps,
    _env: Env,
    info: MessageInfo,
    address: Option<String>,
    denom: Option<String>,
) -> StdResult<LockedTokensResponse> {
    // set `owner` for querying tokens
    let owner: Addr;
    if let Some(val) = address {
        owner = deps.api.addr_validate(&val)?;
    } else {
        owner = info.sender;
    };

    // result contains either a single token for the given denom or all
    // locked tokens for the given owner
    let vtokens: Vec<Coin>;
    if let Some(denom) = denom {
        let res = VTOKENS.load(deps.storage, (owner, &denom))?;
        if res.status != Status::Locked {
            return Err(StdError::GenericErr {
                msg: String::from("No locked tokens for given denom"),
            });
        }
        vtokens = vec![res.vtoken];
    } else {
        vtokens = LOCKED.load(deps.storage, owner)?;
    }

    Ok(LockedTokensResponse { tokens: vtokens })
}

pub fn query_unlocking_tokens(
    deps: Deps,
    _env: Env,
    info: MessageInfo,
    address: Option<String>,
    denom: Option<String>,
) -> StdResult<UnlockingTokensResponse> {
    // set `owner` for querying tokens
    let owner: Addr;
    if let Some(val) = address {
        owner = deps.api.addr_validate(&val)?;
    } else {
        owner = info.sender;
    };

    // result contains either a single token for the given denom or all
    // locked tokens for the given owner
    let vtokens: Vec<Coin>;
    if let Some(denom) = denom {
        let res = VTOKENS.load(deps.storage, (owner, &denom))?;
        if res.status != Status::Unlocking {
            return Err(StdError::GenericErr {
                msg: String::from("No unlocking tokens for given denom"),
            });
        }
        vtokens = vec![res.vtoken];
    } else {
        vtokens = UNLOCKING.load(deps.storage, owner)?;
    }

    Ok(UnlockingTokensResponse { tokens: vtokens })
}

pub fn query_issued_vtokens(
    deps: Deps,
    env: Env,
    info: MessageInfo,
    address: Option<String>,
) -> StdResult<IssuedVtokensResponse> {
    Err(StdError::GenericErr {
        msg: "Not implemented".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{execute, instantiate};
    use crate::msg::{ExecuteMsg, InstantiateMsg};
    use crate::state::{LockingPeriod, PeriodWeight};
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, Decimal, Uint128};

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
            unlock_period: 604_800,
        }
    }

    #[test]
    fn test_get_unlocked_tokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &coins(0, DENOM.to_string()));

        let imsg = init_msg();
        instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

        let msg = ExecuteMsg::Lock {
            app_id: 12,
            locking_period: LockingPeriod::T1,
        };

        let info = mock_info("user1", &coins(100, DENOM.to_string()));

        execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();

        let mut vtoken = VTOKENS
            .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
            .unwrap();
        vtoken.status = Status::Unlocked;
        assert_eq!(vtoken.token.denom, DENOM.to_string());
        assert_eq!(vtoken.status, Status::Unlocked);
        VTOKENS
            .save(
                &mut deps.storage,
                (info.sender.clone(), &info.funds[0].denom),
                &vtoken,
            )
            .unwrap();
        let vtoken = VTOKENS
            .load(&deps.storage, (info.sender.clone(), &info.funds[0].denom))
            .unwrap();

        let res = query_unlocked_tokens(
            deps.as_ref(),
            env,
            info.clone(),
            Option::Some(info.sender.to_string()),
            Option::Some(info.funds[0].denom.to_string()),
        )
        .unwrap();
        // Should get vtokens
        assert_eq!(
            res,
            UnlockedTokensResponse {
                tokens: vec![vtoken.vtoken]
            }
        );
    }
}
