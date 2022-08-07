use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{
    to_binary, Addr, CosmosMsg, CustomQuery, Querier, QuerierWrapper, StdResult, WasmMsg, WasmQuery,
};
use comdex_bindings::{ComdexMessages, ComdexQuery};
use cosmwasm_std::{ Binary, Deps, DepsMut, Env, MessageInfo, Response,Coin};
use comdex_bindings::{
     GetAppResponse, GetAssetDataResponse, MessageValidateResponse, StateResponse,
    TotalSupplyResponse,
};
use cosmwasm_std::{ Decimal,  QueryRequest};

use crate::msg::{ExecuteMsg, QueryMsg};

pub fn query_app_exists(
    deps: Deps<ComdexQuery>,
    app_mapping_id_param: u64,
) -> StdResult<GetAppResponse> {
    let app_info =
        deps.querier
            .query::<GetAppResponse>(&QueryRequest::Custom(ComdexQuery::GetApp {
                app_id: app_mapping_id_param,
            }))?;

    Ok(app_info)
}

pub fn query_get_asset_data(deps: Deps<ComdexQuery>, asset_id_param: u64) -> StdResult<String> {
    let asset_denom = deps
        .querier
        .query::<GetAssetDataResponse>(&QueryRequest::Custom(ComdexQuery::GetAssetData {
            asset_id: asset_id_param,
        }))?;

    Ok(asset_denom.denom)
}
