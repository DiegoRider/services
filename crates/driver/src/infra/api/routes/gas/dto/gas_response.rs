use {crate::domain::eth, crate::util::serialize, serde::Serialize, serde_with::serde_as};

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GasResponse {
    pub gas_estimates: Vec<(u64, SolutionResponse)>,
}

#[serde_as]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SolutionResponse {
    #[serde_as(as = "serialize::U256")]
    pub gas: eth::U256,
    pub success: bool,
    pub message: String,
}
