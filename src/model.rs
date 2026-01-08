#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum AuthorizationPayload {
    String(String),
    Json(serde_json::Value),
}

impl AuthorizationPayload {
    pub fn to_compact_string(&self) -> Result<String, serde_json::Error> {
        match self {
            AuthorizationPayload::String(value) => Ok(value.clone()),
            AuthorizationPayload::Json(value) => serde_json::to_string(value),
        }
    }
}

/// JSON body expected by the `/prove` endpoint.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct ProveRequest {
    /// Primary execution authorization, typically the user's program call.
    pub authorization: AuthorizationPayload,
    /// Optional authorization that produces the fee transition. Supply when the
    /// main execution does not include a fee (e.g. wallets paying on behalf of users).
    #[serde(default)]
    pub fee_authorization: Option<AuthorizationPayload>,
    #[serde(default)]
    pub broadcast: Option<bool>,
    #[serde(default)]
    pub network: Option<crate::config::Network>,
}
