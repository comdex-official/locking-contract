use crate::contract::*;
use crate::error::ContractError;
use crate::helpers::{
    get_token_supply, query_app_exists, query_extended_pair_by_app, query_get_asset_data,
    query_pool_by_app, query_surplus_reward, query_whitelisted_asset,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, SudoMsg};
use std::marker::PhantomData;

use crate::state::{
    Delegation, DelegationInfo, EmissionVaultPool, Proposal, Vote, VotePair, ADMIN,
    APPCURRENTPROPOSAL, BRIBES_BY_PROPOSAL, COMPLETEDPROPOSALS, CSWAP_ID, DELEGATED,
    DELEGATION_INFO, EMISSION, EMISSION_REWARD, MAXPROPOSALCLAIMED, PROPOSAL, PROPOSALCOUNT,
    PROPOSALVOTE, REBASE_CLAIMED, VOTERSPROPOSAL, VOTERS_VOTE,
};
use crate::state::{
    LockingPeriod, PeriodWeight, State, Status, TokenInfo, TokenSupply, Vtoken, STATE, SUPPLY,
    TOKENS, VTOKENS,
};

use comdex_bindings::{ComdexMessages, ComdexQuery};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Addr, Api, BankMsg, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, QueryRequest,
    Response, StdError, StdResult, Storage, Uint128, WasmQuery,
};

use cw2::set_contract_version;
use std::ops::{Div, Mul};

use super::*;
use crate::state::Emission;
use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
use cosmwasm_std::{coin, coins, CosmosMsg, OwnedDeps};

const DENOM: &str = "TKN";
/// Returns default InstantiateMsg with each value in seconds.
/// Thus, t1 is 1 week (7*24*60*60) and similarly, t2 is 2 weeks.
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
        voting_period: 30000,
        vesting_contract: Addr::unchecked("vesting_contract"),
        foundation_addr: vec!["fd1".to_ascii_lowercase(), "fd2".to_ascii_lowercase()],
        foundation_percentage: Decimal::percent(2),
        surplus_asset_id: 3,
        emission: Emission {
            app_id: 1,
            total_rewards: 200000,
            rewards_pending: 200000,
            emission_rate: Decimal::percent(2),
            distributed_rewards: 0,
        },
        admin: Addr::unchecked("admin"),
        min_lock_amount: Uint128::from(1 as u128),
        cswap_id: 1,
    }
}

fn mock_dependencies() -> OwnedDeps<MockStorage, MockApi, MockQuerier, ComdexQuery> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: MockQuerier::default(),
        custom_query_type: PhantomData,
    }
}

#[test]
fn proper_initialization() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let info = mock_info("sender", &coins(0, DENOM.to_string()));

    let msg = init_msg();
    assert_eq!(msg.t1.weight.to_string(), "0.25");

    let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
    assert_eq!(res.messages.len(), 0);
    assert_eq!(res.attributes.len(), 2);

    let state = STATE.load(&deps.storage).unwrap();
    assert_eq!(state.t1, msg.t1);
}

#[test]
fn lock_create_new_nft() {
    // mock values
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("sender", &coins(0, DENOM.to_string()));

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    // Successful execution
    let info = mock_info("user1", &coins(100, DENOM.to_string()));

    //let res = execute(deps.as_mut(), env.clone(), info.clone(), msg.clone()).unwrap();
    let res =
        handle_lock_nft(deps.as_mut(), env.clone(), info, 1, LockingPeriod::T1, None).unwrap();

    assert_eq!(res.messages.len(), 0);
    assert_eq!(res.attributes.len(), 2);

    let sender_addr = Addr::unchecked("user1");
    let token = TOKENS.load(&deps.storage, sender_addr.clone()).unwrap();

    assert_eq!(token.owner, sender_addr.clone());
    assert_eq!(token.token_id, 1u64);
    // .token should be the same as locked tokens

    // Check to see the SUPPLY mapping is correct
    let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
    assert_eq!(supply.token, 100u128);
    assert_eq!(supply.vtoken, 25u128);
}

#[test]
fn lock_different_denom_and_time_period() {
    // mock values
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("sender", &coins(0, DENOM.to_string()));

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let info = mock_info("owner", &coins(100, "DNM1".to_string()));
    let owner_addr = Addr::unchecked("owner");

    // Create a new entry for DENOM in nft
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("owner", &coins(100, "DNM2".to_string()));
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T2,
        None,
    )
    .unwrap();

    // Check correct update in TOKENS

    let nft = VTOKENS
        .load(deps.as_ref().storage, (owner_addr.clone(), "DNM1"))
        .unwrap();
    let nft2 = VTOKENS
        .load(deps.as_ref().storage, (owner_addr.clone(), "DNM2"))
        .unwrap();

    assert_eq!(nft.len(), 1);
    assert_eq!(nft[0].token.denom, "DNM1".to_string());
    assert_eq!(nft2[0].token.denom, "DNM2".to_string());

    // Check correct update in SUPPLY
    let supply = SUPPLY.load(deps.as_ref().storage, "DNM1").unwrap();
    assert_eq!(supply.token, 100u128);
    assert_eq!(supply.vtoken, 25u128);

    let supply = SUPPLY.load(deps.as_ref().storage, "DNM2").unwrap();
    assert_eq!(supply.token, 100u128);
    assert_eq!(supply.vtoken, 50u128);
}

#[test]
fn lock_same_denom_and_time_period() {
    // mock values
    let mut deps = mock_dependencies();
    let mut env = mock_env();
    let info = mock_info("sender", &[]);

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let info = mock_info("owner", &coins(100, DENOM.to_string()));
    let owner_addr = Addr::unchecked("owner");

    // Create a new entry for DENOM in nft
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    // forward the time, inside 1 week
    let old_start_time = env.block.time;
    env.block.time = env.block.time.plus_seconds(100_000);

    let info = mock_info("owner", &coins(100, DENOM.to_string()));
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    // Check correct updating in nft
    let nft = VTOKENS
        .load(deps.as_ref().storage, (owner_addr.clone(), "TKN"))
        .unwrap();

    assert_eq!(nft.len(), 2);
    assert_eq!(nft[0].token.amount.u128(), 100u128);
    assert_eq!(nft[0].vtoken.amount.u128(), 25u128);
    assert_eq!(nft[0].start_time, old_start_time);
    assert_eq!(nft[0].end_time, old_start_time.plus_seconds(imsg.t1.period));
    assert_eq!(nft[0].period, LockingPeriod::T1);
    assert_eq!(nft[0].status, Status::Locked);

    // Check correct update in SUPPLY
    let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
    assert_eq!(supply.vtoken, 50u128);
    assert_eq!(supply.token, 200u128);
}

#[test]
fn lock_same_denom_diff_time_period() {
    // mock values
    let mut deps = mock_dependencies();
    let mut env = mock_env();
    let info = mock_info("sender", &coins(0, DENOM.to_string()));

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let info = mock_info("owner", &coins(100, DENOM.to_string()));
    let owner_addr = Addr::unchecked("owner");

    // Create a new entry for DENOM in nft
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    // forward the time, inside 1 week
    env.block.time = env.block.time.plus_seconds(100_000);

    // Lock for new time period
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T2,
        None,
    )
    .unwrap();

    // Check correct updating in nft
    let nft = VTOKENS
        .load(deps.as_ref().storage, (owner_addr.clone(), "TKN"))
        .unwrap();

    assert_eq!(nft.len(), 2);
    assert_eq!(nft[0].token.amount.u128(), 100u128);
    assert_eq!(nft[0].vtoken.amount.u128(), 25u128);
    assert_eq!(nft[1].token.amount.u128(), 100u128);
    assert_eq!(nft[1].vtoken.amount.u128(), 50u128);
    assert_eq!(nft[1].start_time, env.block.time);
    assert_eq!(nft[1].end_time, env.block.time.plus_seconds(imsg.t2.period));
    assert_eq!(nft[0].period, LockingPeriod::T1);
    assert_eq!(nft[0].status, Status::Locked);
    assert_eq!(nft[1].period, LockingPeriod::T2);
    assert_eq!(nft[1].status, Status::Locked);

    // Check correct update in SUPPLY
    let supply = SUPPLY.load(deps.as_ref().storage, &DENOM).unwrap();
    assert_eq!(supply.token, 200u128);
    assert_eq!(supply.vtoken, 75u128);
}

#[test]
fn lock_zero_transfer() {
    // mock values
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("sender", &coins(0, DENOM.to_string()));

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    // This should throw an error because the amount is zero
    let res = handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        10,
        LockingPeriod::T1,
        None,
    )
    .unwrap_err();
    match res {
        ContractError::InsufficientFunds { .. } => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn withdraw_basic_functionality() {
    // mock values
    let mut deps = mock_dependencies();
    let mut env = mock_env();
    let info = mock_info("sender", &[]);

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let _owner = Addr::unchecked("owner");
    let info = mock_info("owner", &coins(100, DENOM.to_string()));

    let _res = handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    env.block.time = env.block.time.plus_seconds(imsg.t1.period + 1u64);

    // Withdrawing 10 Tokens
    let info = mock_info("owner", &[]);
    let res = handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string()).unwrap();
    assert_eq!(res.messages.len(), 1);
    assert_eq!(res.attributes.len(), 2);
    assert_eq!(
        res.messages[0].msg,
        CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![coin(100, DENOM.to_string())]
        })
    );

    // Just a check if the order matters with Decimal
    let vtoken_balance1 = Uint128::from(90u64) * imsg.t1.weight;
    let vtoken_balance2 = imsg.t1.weight * Uint128::from(90u64);
    assert_eq!(vtoken_balance1, vtoken_balance2);
}

#[test]
fn withdraw_no_vtokens() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("sender", &[]);

    let res =
        handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string()).unwrap_err();
    match res {
        ContractError::NotFound { .. } => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn withdraw_not_unlocked() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let res = handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap_err();
    match res {
        ContractError::InsufficientFunds { .. } => {}
        e => panic!("{:?}", e),
    };
    let _owner = Addr::unchecked("owner");
    let _info = mock_info("owner", &coins(100, DENOM.to_string()));
    let res =
        handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string()).unwrap_err();
    match res {
        ContractError::NotFound { .. } => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn withdraw_period_not_locked() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let _owner = Addr::unchecked("owner");
    let info = mock_info("owner", &coins(100, DENOM.to_string()));
    let _res = handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("owner", &[]);
    let res =
        handle_withdraw(deps.as_mut(), env.clone(), info.clone(), DENOM.to_string()).unwrap_err();
    match res {
        ContractError::NotFound { .. } => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn withdraw_denom_not_locked() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    let _owner = Addr::unchecked("owner");
    let info = mock_info("owner", &coins(100, DENOM.to_string()));
    let _res = handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("owner", &[]);
    let res =
        handle_withdraw(deps.as_mut(), env.clone(), info.clone(), "DNM1".to_string()).unwrap_err();
    match res {
        ContractError::NotFound { .. } => {}
        e => panic!("{:?}", e),
    };
}

// #[test]
// fn transfer_different_denom() {
//     let mut deps = mock_dependencies();
//     let env = mock_env();
//     let info = mock_info("owner", &[]);

//     let imsg = init_msg();
//     instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

//     let owner = Addr::unchecked("owner");
//     let recipient = Addr::unchecked("recipient");

//     let denom1 = "DNM1";
//     let denom2 = "DNM2";

//     // Create token for recipient
//     let info = mock_info("recipient", &coins(100, denom2.to_string()));
//     handle_lock_nft(
//         deps.as_mut(),
//         env.clone(),
//         info,
//         12,
//         LockingPeriod::T1,
//         None,
//     )
//     .unwrap();

//     // Create tokens for owner == sender
//     let info = mock_info("owner", &coins(100, denom1.to_string()));
//     handle_lock_nft(
//         deps.as_mut(),
//         env.clone(),
//         info.clone(),
//         12,
//         LockingPeriod::T1,
//         None,
//     )
//     .unwrap();

//     // create a copy of owner's vtoken to compare and check if the recipient's
//     // vtoken is the same.
//     let locked_vtokens = VTOKENS
//         .load(deps.as_ref().storage, (owner.clone(), denom1))
//         .unwrap();

//     let msg = ExecuteMsg::Transfer {
//         recipient: recipient.to_string(),
//         locking_period: LockingPeriod::T1,
//         denom: denom1.to_string(),
//     };

//     let info = mock_info(owner.as_str(), &[]);
//     let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
//     assert_eq!(res.messages.len(), 0);
//     assert_eq!(res.attributes.len(), 3);

//     // Check correct update in sender vtokens
//     let res = VTOKENS
//         .load(deps.as_ref().storage, (owner.clone(), denom1))
//         .unwrap_err();
//     match res {
//         StdError::NotFound { .. } => {}
//         e => panic!("{:?}", e),
//     }

//     // Check correct update in recipient vtokens
//     {
//         let res = VTOKENS
//             .load(deps.as_ref().storage, (recipient.clone(), denom1))
//             .unwrap();
//         assert_eq!(res.len(), 1);
//         assert_eq!(res[0], locked_vtokens[0]);

//         let res = VTOKENS
//             .load(deps.as_ref().storage, (recipient.clone(), denom2))
//             .unwrap();
//         assert_eq!(res.len(), 1);
//         assert_eq!(res[0].token.amount.u128(), 100);
//         assert_eq!(res[0].token.denom, denom2.to_string());
//     }

//     // Check correct update in recipient nft
//     let recipient_nft = VTOKENS
//         .load(deps.as_ref().storage, (recipient.clone(), "DNM1"))
//         .unwrap();

//     assert_eq!(recipient_nft.len(), 1);
//     assert_eq!(recipient_nft[0].token.amount.u128(), 100);
//     assert_eq!(recipient_nft[0].token.denom, denom1.to_string());
// }

#[test]
fn transfer_same_denom() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("admin", &[]);

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();

    let sender = Addr::unchecked("sender");
    let recipient = Addr::unchecked("recipient");

    // Lock tokens for sender
    let info = mock_info(sender.as_str(), &coins(1000, DENOM.to_string()));
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info,
        12,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    // Lock tokens for recipient
    let info = mock_info(recipient.as_str(), &coins(1000, DENOM.to_string()));
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info,
        12,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    // Transfer tokens to recipient
    let info = mock_info(sender.as_str(), &[]);
    handle_transfer(
        deps.as_mut(),
        env.clone(),
        info,
        recipient.to_string(),
        LockingPeriod::T1,
        DENOM.to_string(),
    )
    .unwrap();

    // Check VTOKENS
    VTOKENS
        .load(deps.as_ref().storage, (sender.clone(), DENOM))
        .unwrap_err();

    let recipient_vtokens = VTOKENS
        .load(deps.as_ref().storage, (recipient.clone(), DENOM))
        .unwrap();
    assert_eq!(recipient_vtokens.len(), 2);
    assert_eq!(recipient_vtokens[0].token.amount.u128(), 1000);
    assert_eq!(recipient_vtokens[0].token.denom, DENOM.to_string());
    assert_eq!(recipient_vtokens[0].vtoken.amount.u128(), 250);
    assert_eq!(recipient_vtokens[0].vtoken.denom, "vTKN".to_string());
    assert_eq!(recipient_vtokens[0].start_time, env.block.time);
    assert_eq!(
        recipient_vtokens[0].end_time,
        env.block.time.plus_seconds(imsg.t1.period)
    );
    assert_eq!(recipient_vtokens[1].token.amount.u128(), 1000);
    assert_eq!(recipient_vtokens[1].vtoken.amount.u128(), 250);
}

#[test]
fn raise_init() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("sender", &[]);

    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info, imsg.clone()).unwrap();
}

#[test]
fn test_non_admin_proposal_error() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("sender", &[]);

    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
    let res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap_err();
    match res {
        ContractError::Admin { .. } => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn proposal_successfully_set() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("admin", &[]);

    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
    let _res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
    let current_proposal = APPCURRENTPROPOSAL.load(deps.as_ref().storage, 1).unwrap();
    assert_eq!(current_proposal, 1);
}

#[test]
fn no_consecutive_proposals() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("admin", &[]);

    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
    let _res = raise_proposal(deps.as_mut(), env.clone(), info.clone(), 1, vec![1, 2, 3]).unwrap();
    let res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap_err();
    match res {
        ContractError::CustomError { .. } => {}
        e => panic!("{:?}", e),
    };
    let current_proposal = APPCURRENTPROPOSAL.load(deps.as_ref().storage, 1).unwrap();
    assert_eq!(current_proposal, 1);
}

#[test]
fn no_pair_for_proposals() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("admin", &[]);

    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();
    let res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![]).unwrap_err();
    match res {
        ContractError::CustomError { .. } => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn test_vote_proposal_without_locking() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    let _res = raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info("voter1", &[]);

    let err = vote_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        vec![1],
        DENOM.to_string(),
        vec![Decimal::from_ratio(
            Uint128::from(100_u128),
            Uint128::from(200_u128),
        )],
    );
    assert_eq!(
        err,
        Err(ContractError::CustomError {
            val: "No tokens locked to perform voting on proposals".to_string()
        })
    );
}

#[test]
fn test_vote_proposal_with_wrong_extended_pair() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info("voter1", &[]);

    let err = vote_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        vec![6],
        DENOM.to_string(),
        vec![Decimal::from_ratio(
            Uint128::from(100_u128),
            Uint128::from(200_u128),
        )],
    );
    assert_eq!(
        err,
        Err(ContractError::CustomError {
            val: "Extended pair does not exist in proposal".to_string()
        })
    );
}

#[test]
fn test_bribe_proposal() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info(
        "voter1",
        &[Coin {
            amount: Uint128::from(200_u128),
            denom: "bribe1".to_string(),
        }],
    );

    bribe_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        Coin {
            amount: Uint128::from(200_u128),
            denom: "bribe1".to_string(),
        },
    )
    .unwrap();
}

#[test]
fn test_bribe_proposal_wrong_pair() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info(
        "voter1",
        &[Coin {
            amount: Uint128::from(200_u128),
            denom: "bribe1".to_string(),
        }],
    );

    let err = bribe_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        6,
        Coin {
            amount: Uint128::from(200_u128),
            denom: "bribe1".to_string(),
        },
    );
    assert_eq!(
        err,
        Err(ContractError::CustomError {
            val: "Invalid Extended pair".to_string()
        })
    );
}

#[test]
fn test_bribe_proposal_multiple_denoms() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info(
        "voter1",
        &[
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe2".to_string(),
            },
        ],
    );

    let err = bribe_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        Coin {
            amount: Uint128::from(200_u128),
            denom: "bribe1".to_string(),
        },
    );
    assert_eq!(
        err,
        Err(ContractError::CustomError {
            val: "Multiple denominations are not supported".to_string()
        })
    );
}

#[test]
fn test_foundation_emission_no_completed() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info(
        "admin",
        &[
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe2".to_string(),
            },
        ],
    );

    let err = emission_foundation(deps.as_mut(), env.clone(), info.clone(), 1);
    assert_eq!(
        err,
        Err(ContractError::CustomError {
            val: "Emission calculation did not take place to initiate foundation calculation"
                .to_string()
        })
    );
}

#[test]
fn test_foundation_emission_accept_no_fund() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
    let mut proposal = PROPOSAL.load(deps.as_ref().storage, 1).unwrap();
    proposal.emission_completed = true;
    proposal.foundation_distributed = 10000000000;
    _ = PROPOSAL.save(deps.as_mut().storage, 1, &proposal);
    let info = mock_info(
        "admin",
        &[
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe1".to_string(),
            },
            Coin {
                amount: Uint128::from(200_u128),
                denom: "bribe2".to_string(),
            },
        ],
    );

    let err = emission_foundation(deps.as_mut(), env.clone(), info.clone(), 1);
    assert_eq!(
        err,
        Err(ContractError::CustomError {
            val: "Funds not accepted".to_string()
        })
    );
}

#[test]
fn test_foundation_emission() {
    // Mock dependencies
    let mut deps = mock_dependencies();
    let env = mock_env();

    let info = mock_info("voter1", &coins(100, DENOM.to_string()));
    // Initialize
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
    let mut proposal = PROPOSAL.load(deps.as_ref().storage, 1).unwrap();
    proposal.emission_completed = true;
    proposal.foundation_distributed = 10000000000;
    _ = PROPOSAL.save(deps.as_mut().storage, 1, &proposal);
    let info = mock_info("admin", &[]);

    let _response = emission_foundation(deps.as_mut(), env.clone(), info.clone(), 1);
}

// #[test]
// fn test_bribe_reward() {
//     // Mock dependencies
//     let mut deps = mock_dependencies();
//     let env = mock_env();

//     let info = mock_info("voter1", &coins(100, DENOM.to_string()));
//     // Initialize
//     let imsg = init_msg();
//     instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

//     handle_lock_nft(
//         deps.as_mut(),
//         env.clone(),
//         info.clone(),
//         1,
//         LockingPeriod::T1,
//         None,
//     )
//     .unwrap();

//     let info = mock_info("admin", &[]);
//     raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();
//     let mut proposal = PROPOSAL.load(deps.as_ref().storage, 1).unwrap();
//     proposal.emission_completed = true;
//     proposal.foundation_distributed = 10000000000;
//     _ = PROPOSAL.save(deps.as_mut().storage, 1, &proposal);
//     let info = mock_info(
//         "voter1",
//         &[Coin {
//             amount: Uint128::from(200_u128),
//             denom: "bribe1".to_string(),
//         }],
//     );

//     bribe_proposal(
//         deps.as_mut(),
//         env.clone(),
//         info.clone(),
//         1,
//         1,
//         Coin {
//             amount: Uint128::from(200_u128),
//             denom: "bribe1".to_string(),
//         },
//     )
//     .unwrap();

//     let vote = Vote {
//         app_id: 1,
//         extended_pair: 1,
//         vote_weight: 200,
//     };
//     _ = VOTERSPROPOSAL.save(deps.as_mut().storage, (Addr::unchecked("voter1"), 1), &vote);
//     _ = PROPOSALVOTE.save(deps.as_mut().storage, (1, 1), &Uint128::from(500_u128));

//     let info = mock_info("voter1", &[]);

//     let response =
//         calculate_bribe_reward(deps.as_ref(), env.clone(), info.clone(), 0, vec![1], 1)
//             .unwrap();
//     assert_eq!(
//         response,
//         vec![Coin {
//             denom: "bribe1".to_ascii_lowercase(),
//             amount: Uint128::from(80_u128)
//         }]
//     );
// }

#[test]
fn test_rebase_formula() {
    let total_locked: u128 = 10000_u128;

    let my_locked: u128 = 222_u128;

    let rebase_amount: u128 = 20000_u128;

    let lock_amount_t1 = Uint128::from(my_locked).mul(Decimal::from_ratio(
        Uint128::from(rebase_amount),
        Uint128::from(total_locked),
    ));

    assert_eq!(lock_amount_t1, Uint128::new(444_u128));
}

#[test]
fn test_bribe_formula() {
    let vote_weight: u128 = 30_u128;

    let total_vote: u128 = 100_u128;

    let claimable_amount = (Decimal::new(Uint128::from(vote_weight))
        .div(Decimal::new(Uint128::from(total_vote))))
    .mul(Uint128::from(500_u128));

    assert_eq!(claimable_amount, Uint128::new(150_u128));
}

#[test]
fn instantiate_multi_foundation_addr() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let info = mock_info("owner", &[]);

    let mut imsg = init_msg();
    imsg.foundation_addr = vec![
        "fd1".to_string(),
        "fd2".to_ascii_lowercase(),
        "fd3".to_ascii_lowercase(),
        "fd2".to_string(),
    ];
    instantiate(deps.as_mut(), env.clone(), info, imsg).unwrap();

    let state = STATE.load(deps.as_ref().storage).unwrap();
    assert_eq!(state.foundation_addr.len(), 3);
    assert_eq!(
        state.foundation_addr,
        vec![
            "fd1".to_string(),
            "fd2".to_ascii_lowercase(),
            "fd3".to_ascii_lowercase()
        ]
    );
}

#[test]
fn instantiate_foundation_addr_multiple_encodes() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let info = mock_info("owner", &[]);

    let mut imsg = init_msg();
    imsg.foundation_addr = vec![
        "fd1".to_string(),
        "fd2".to_ascii_lowercase(),
        "fd2".to_string(),
        "fd2".to_lowercase(),
        "fd3".to_string(),
    ];
    instantiate(deps.as_mut(), env.clone(), info, imsg).unwrap();

    let state = STATE.load(deps.as_ref().storage).unwrap();
    assert_eq!(state.foundation_addr.len(), 3);
    assert_eq!(
        state.foundation_addr,
        vec![
            "fd1".to_string(),
            "fd2".to_string(),
            "fd3".to_ascii_lowercase()
        ]
    );
}

#[test]
fn lock_funds_and_vote() {
    // Mock
    let mut env = mock_env();
    let mut deps = mock_dependencies();
    let info = mock_info("user1", &coins(100, DENOM));

    // Instantitate
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    // Lock funds
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T1,
        None,
    )
    .unwrap();

    let user1_vtokens = VTOKENS
        .load(deps.as_ref().storage, (info.sender.clone(), DENOM))
        .unwrap();
    assert_eq!(user1_vtokens.len(), 1);
    assert_eq!(user1_vtokens[0].token, coin(100, DENOM));
    assert_eq!(user1_vtokens[0].vtoken, coin(25, "vTKN"));

    env.block.height += 1;

    // Raise proposal
    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    let info = mock_info("user1", &[]);
    let result = vote_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        vec![1],
        DENOM.to_string(),
        vec![Decimal::from_ratio(
            Uint128::from(100_u128),
            Uint128::from(200_u128),
        )],
    )
    .unwrap();
    assert_eq!(result.messages.len(), 0);
}

#[test]
fn funds_sent_during_raise_proposal() {
    let env = mock_env();
    let mut deps = mock_dependencies();
    let info = mock_info("admin", &coins(100, DENOM));

    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg).unwrap();

    let result =
        raise_proposal(deps.as_mut(), env.clone(), info.clone(), 1, vec![1, 2, 3]).unwrap_err();
    println!("Now {:?} will print!", result);
    println!("Now {:?} will print!", info);

    match result {
        ContractError::FundsNotAllowed {} => {}
        e => panic!("{:?}", e),
    };
}

#[test]
fn pools_split() {
    let ext_pair: Vec<u64> = vec![1, 2, 3, 4, 1000001, 1000002, 1000003, 1000004];
    let mut extd_pair: Vec<u64> = vec![];
    let mut pool_ids: Vec<u64> = vec![];
    for i in ext_pair {
        if Uint128::from(i).div(Uint128::from(1000000_u64)) == Uint128::zero() {
            extd_pair.push(i);
        } else {
            let pool_id = i % 1000000;
            pool_ids.push(pool_id);
        }
    }
    assert_eq!(extd_pair, vec![1, 2, 3, 4]);
    assert_eq!(pool_ids, vec![1, 2, 3, 4]);
}

#[test]
fn emission_split() {
    let emission_distributed: u128 = 10000000;
    let total_vote: Uint128 = Uint128::from(500_u128);
    let pool_vote: Uint128 = Uint128::from(400_u128);
    let pools_share = pool_vote
        .mul(Uint128::from(emission_distributed))
        .div(total_vote);
    let vault_share = Uint128::from(emission_distributed) - pools_share;
    assert_eq!(pools_share, Uint128::from(8000000_u128));
    assert_eq!(vault_share, Uint128::from(2000000_u128));
}

#[test]
fn test_change_vote() {
    let mut env = mock_env();
    let mut deps = mock_dependencies();
    let info = mock_info("user1", &coins(100, DENOM));

    // Instantitate
    let imsg = init_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), imsg.clone()).unwrap();

    // Lock funds
    handle_lock_nft(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        LockingPeriod::T2,
        None,
    )
    .unwrap();

    env.block.height += 1;

    // Raise proposal
    let info = mock_info("admin", &[]);
    raise_proposal(deps.as_mut(), env.clone(), info, 1, vec![1, 2, 3]).unwrap();

    // Vote on proposal
    let info = mock_info("user1", &[]);
    let result = vote_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        vec![1],
        DENOM.to_string(),
        vec![Decimal::from_ratio(
            Uint128::from(100_u128),
            Uint128::from(200_u128),
        )],
    )
    .unwrap();
    assert_eq!(result.messages.len(), 0);

    let vote_weight = VOTERSPROPOSAL
        .load(deps.as_ref().storage, (info.sender.clone(), 1))
        .unwrap();
    assert_eq!(vote_weight.votes[0].extended_pair, 1);
    assert_eq!(vote_weight.votes[0].vote_weight, 25u128);

    // Vote on a different pair
    env.block.height += 1;
    env.block.time = env.block.time.plus_seconds(10);

    let _result = vote_proposal(
        deps.as_mut(),
        env.clone(),
        info.clone(),
        1,
        1,
        vec![2],
        DENOM.to_string(),
        vec![Decimal::from_ratio(
            Uint128::from(100_u128),
            Uint128::from(200_u128),
        )],
    )
    .unwrap();
    let vote_weight = VOTERSPROPOSAL
        .load(deps.as_ref().storage, (info.sender.clone(), 1))
        .unwrap();
    assert_eq!(vote_weight.votes[0].extended_pair, 2);
    assert_eq!(vote_weight.votes[0].vote_weight, 25u128);
}
