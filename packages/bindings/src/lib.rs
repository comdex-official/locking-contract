mod msg;
mod query;

pub use msg::ComdexMessages;
pub use query::{
    ComdexQuery, GetAppResponse, GetAssetDataResponse, MessageValidateResponse, StateResponse,
    TotalSupplyResponse,
};

// This is a signal, such that any contract that imports these helpers will only run on the
// comdex blockchain
#[no_mangle]
extern "C" fn requires_comdex() {}
