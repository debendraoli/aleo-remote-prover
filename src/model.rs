#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct ProveRequest {
    pub authorization: serde_json::Value,
    #[serde(default)]
    pub fee_authorization: Option<serde_json::Value>,
    #[serde(default)]
    pub broadcast: Option<bool>,
}
