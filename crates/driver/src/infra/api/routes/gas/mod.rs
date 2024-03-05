use {
    crate::infra::{
        api::{Error, State}, //Error,
    },
    tracing::Instrument,
};

mod dto;
pub use dto::GasError;
pub use dto::GasRequest;

pub(in crate::infra::api) fn gas(app: axum::Router<State>) -> axum::Router<State> {
    app.route("/gas", axum::routing::post(route))
}

async fn route(
    state: axum::extract::State<State>,
    gas_request: axum::Json<GasRequest>,
) -> Result<axum::Json<dto::GasResponse>, (hyper::StatusCode, axum::Json<Error>)> {
    let competition = state.competition();
    let auction_id = competition.auction_id().map(|id| id.0);

    let handle_request = async {
        let (auction, solutions) = gas_request
            .0
            .into_domain(
                &state.0,
                competition.eth.contracts().weth_address(),
                competition.solver.clone(),
            )
            .await?;

        tracing::trace!("estimating gas");
        let result = competition.estimate_gas(&auction, solutions).await;
        tracing::info!("estimated gas");
        let estimates = result?
            .into_iter()
            .map(|(solution_id, (gas, success, message))| {
                (
                    solution_id,
                    dto::SolutionResponse {
                        gas,
                        success,
                        message,
                    },
                )
            })
            .collect();
        Ok(axum::Json(dto::GasResponse {
            gas_estimates: estimates,
        }))
    };

    handle_request
        .instrument(tracing::info_span!("/gas", solver = %state.solver().name(), auction_id))
        .await
}
