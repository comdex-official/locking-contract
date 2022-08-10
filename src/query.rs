// !------- IssuedVtokens query not implemented-------!

use comdex_bindings::ComdexQuery;
use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Coin, Deps, Env, MessageInfo, StdError, StdResult,
};

use crate::msg::{
    IssuedNftResponse, IssuedVtokensResponse, LockedTokensResponse, QueryMsg,
    UnlockedTokensResponse,
};
use crate::state::{Status, LOCKED, TOKENS, UNLOCKED, VTOKENS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(
    deps: Deps<ComdexQuery>,
    env: Env,
    info: MessageInfo,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::IssuedNft { address } => to_binary(&query_issued_nft(deps, env, info, address)?),

        QueryMsg::UnlockedTokens { address, denom } => {
            to_binary(&query_unlocked_tokens(deps, env, info, address, denom)?)
        }

        QueryMsg::LockedTokens { address, denom } => {
            to_binary(&query_locked_tokens(deps, env, info, address, denom)?)
        }

        QueryMsg::IssuedVtokens { address } => {
            to_binary(&query_issued_vtokens(deps, env, info, address)?)
        }

        _ => panic!("Not implemented"),
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

pub fn query_unlocked_tokens(
    deps: Deps<ComdexQuery>,
    _env: Env,
    info: MessageInfo,
    address: Option<String>,
    denom: Option<String>,
) -> StdResult<UnlockedTokensResponse> {
    // set `owner` for querying tokens
    let owner = if let Some(val) = address {
        deps.api.addr_validate(&val)?
    } else {
        info.sender
    };

    // result contains either a single token for the given denom or all
    // unlocked tokens for the given owner
    let tokens = UNLOCKED.may_load(deps.storage, owner)?;

    let mut unlocking_tokens = if let Some(val) = tokens {
        val
    } else {
        return Err(StdError::NotFound {
            kind: "No unlocked tokens".into(),
        });
    };

    unlocking_tokens = if let Some(val) = denom {
        unlocking_tokens
            .into_iter()
            .filter(|el| el.denom == val)
            .collect()
    } else {
        unlocking_tokens
    };

    Ok(UnlockedTokensResponse {
        tokens: unlocking_tokens,
    })
}

pub fn query_locked_tokens(
    deps: Deps<ComdexQuery>,
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
    let tokens = LOCKED.may_load(deps.storage, owner)?;

    let mut locked_tokens = if let Some(val) = tokens {
        val
    } else {
        return Err(StdError::NotFound {
            kind: "No locked tokens".into(),
        });
    };

    locked_tokens = if let Some(val) = denom {
        locked_tokens
            .into_iter()
            .filter(|el| el.denom == val)
            .collect()
    } else {
        locked_tokens
    };

    Ok(LockedTokensResponse {
        tokens: locked_tokens,
    })
}

pub fn query_issued_vtokens(
    deps: Deps<ComdexQuery>,
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
    use comdex_bindings::ComdexQuery;
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{coins, Decimal, OwnedDeps, Uint128};
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
    fn unlocked_tokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &coins(0, DENOM.to_string()));

        let owner = Addr::unchecked("owner");

        // Save some tokens in UNLOCKED
        let unlocked_tokens = vec![
            Coin {
                amount: Uint128::from(1000u128),
                denom: "DNM1".to_string(),
            },
            Coin {
                amount: Uint128::from(2000u128),
                denom: "DNM2".to_string(),
            },
        ];
        UNLOCKED
            .save(deps.as_mut().storage, owner.clone(), &unlocked_tokens)
            .unwrap();

        // Query unlocked tokens for specific denom
        let res = query_unlocked_tokens(
            deps.as_ref(),
            env.clone(),
            info.clone(),
            Some(owner.to_string()),
            Some("DNM1".to_string()),
        )
        .unwrap();

        assert_eq!(res.tokens.len(), 1);
        assert_eq!(res.tokens[0].amount.u128(), 1000);
        assert_eq!(res.tokens[0].denom, "DNM1".to_string());

        // Query all tokens
        let res =
            query_unlocked_tokens(deps.as_ref(), env.clone(), info.clone(), None, None).unwrap();
        assert_eq!(res.tokens.len(), 2);
        assert_eq!(res.tokens[0].denom, "DNM1".to_string());
        assert_eq!(res.tokens[0].amount.u128(), 1000u128);
        assert_eq!(res.tokens[1].denom, "DNM2".to_string());
        assert_eq!(res.tokens[1].amount.u128(), 2000u128);
    }

    #[test]
    fn locked_tokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("owner", &coins(0, "DNM1".to_string()));

        let owner = Addr::unchecked("owner");

        let locked_tokens = coins(1000, "DNM1".to_string());
        LOCKED
            .save(deps.as_mut().storage, owner.clone(), &locked_tokens)
            .unwrap();

        let res = query_locked_tokens(
            deps.as_ref(),
            env.clone(),
            info.clone(),
            Some(owner.to_string()),
            Some("DNM1".into()),
        )
        .unwrap();
        assert_eq!(res.tokens.len(), 1);
        assert_eq!(res.tokens[0].amount.u128(), 1000);
        assert_eq!(res.tokens[0].denom, "DNM1".to_string());
    }
}
