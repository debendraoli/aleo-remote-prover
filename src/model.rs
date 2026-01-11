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

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct ProveRequest {
    pub authorization: AuthorizationPayload,
    #[serde(default)]
    pub fee_authorization: Option<AuthorizationPayload>,
    #[serde(default)]
    pub broadcast: Option<bool>,
}
