
use cosmwasm_std::{StdResult};
use comdex_bindings::{ComdexQuery};
use cosmwasm_std::{  Deps, Coin};
use comdex_bindings::{
     GetAppResponse, GetAssetDataResponse,TotalSupplyResponse,GetExtendedPairByAppResponse,GetSurplusRewardAmount,GetWhitelistedAssetResponse,GetPoolByAppResponse};
use cosmwasm_std::{ QueryRequest};

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

/// get token_supply of an asset at current height
pub fn get_token_supply(
    deps: Deps<ComdexQuery>,
    app_id_param: u64,
    asset_id_param: u64,
) -> StdResult<u64> {
    let total_token_supply = deps
        .querier
        .query::<TotalSupplyResponse>(&QueryRequest::Custom(ComdexQuery::TotalSupply {
            app_id: app_id_param,
            asset_id: asset_id_param,
        }))?;

    Ok(total_token_supply.current_supply)
}

pub fn get_token_vote_weight(
    deps: Deps<ComdexQuery>,
    app_id_param: u64,
    asset_id_param: u64,
) -> StdResult<u64> {
    let total_token_supply = deps
        .querier
        .query::<TotalSupplyResponse>(&QueryRequest::Custom(ComdexQuery::TotalSupply {
            app_id: app_id_param,
            asset_id: asset_id_param,
        }))?;

    Ok(total_token_supply.current_supply)
}

pub fn query_extended_pair_by_app(
    deps: Deps<ComdexQuery>,
    app_mapping_id_param: u64,
) -> StdResult<Vec<u64>> {
    let ext_pair_response =
        deps.querier
            .query::<GetExtendedPairByAppResponse>(&QueryRequest::Custom(ComdexQuery::ExtendedPairByApp {
                app_id: app_mapping_id_param,
            }))?;

    Ok(ext_pair_response.ext_pair)
}

pub fn query_pool_by_app(
    deps: Deps<ComdexQuery>,
    app_mapping_id_param: u64,
) -> StdResult<Vec<u64>> {
    let pool_pair =
        deps.querier
            .query::<GetPoolByAppResponse>(&QueryRequest::Custom(ComdexQuery::PoolByApp  {
                app_id: app_mapping_id_param,
            }))?;

    Ok(pool_pair.pools)
}


pub fn query_surplus_reward(
    deps: Deps<ComdexQuery>,
    app_mapping_id_param: u64,
    asset_id:u64,
) -> StdResult<Coin> {
    let amount =
        deps.querier
            .query::<GetSurplusRewardAmount>(&QueryRequest::Custom(ComdexQuery::CheckSurplusReward {
                app_id: app_mapping_id_param,
                asset_id
            }))?;

    Ok(amount.amount)
}

pub fn query_whitelisted_asset(
    deps: Deps<ComdexQuery>,
    asset_denom:String,
) -> StdResult<bool> {
    let response =
        deps.querier
            .query::<GetWhitelistedAssetResponse>(&QueryRequest::Custom(ComdexQuery::CheckWhitelistedAsset {
                denom:asset_denom
            }))?;

    Ok(response.found)
}