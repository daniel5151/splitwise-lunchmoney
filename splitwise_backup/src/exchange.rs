use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

/// A full record of an HTTP request and response cycle.
///
/// Designed to be replayable by a local mock Splitwise API server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpExchange {
    /// Sequential ID and human-readable descriptor (e.g. `0001_GET_get_current_user`)
    pub exchange_id: String,
    /// Request details
    pub request: HttpRequestRecord,
    /// Response details including headers and raw body
    pub response: HttpResponseRecord,
    /// Timing and execution metadata
    pub metadata: ExchangeMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestRecord {
    pub method: String,
    pub url: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponseRecord {
    pub status: u16,
    pub status_text: String,
    pub headers: BTreeMap<String, String>,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeMetadata {
    pub timestamp: String,
    pub duration_ms: u64,
    pub byte_size: usize,
}
