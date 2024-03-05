mod gas_request;
mod gas_response;

pub use {
    gas_request::{Error as GasError, GasRequest},
    gas_response::{GasResponse, SolutionResponse},
};
