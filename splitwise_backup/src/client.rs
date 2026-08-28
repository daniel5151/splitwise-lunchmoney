use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use anstream::eprintln;
use anstream::println;
use anyhow::Context;
use lm_common::style::*;
use reqwest::header::HeaderMap;
use tokio::sync::Semaphore;

use crate::exchange::ExchangeMetadata;
use crate::exchange::HttpExchange;
use crate::exchange::HttpRequestRecord;
use crate::exchange::HttpResponseRecord;

/// A raw HTTP client for the Splitwise API that logs all HTTP transactions
/// for mock server replay, with automatic rate limit backoff and retries.
pub struct RawClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    exchanges_dir: PathBuf,
    exchange_counter: AtomicUsize,
    redact_api_key: bool,
    semaphore: Arc<Semaphore>,
    inter_request_delay: Duration,
    verbose: bool,
    max_retries: u32,
    initial_delay: Duration,
}

impl RawClient {
    pub fn new(
        http: reqwest::Client,
        api_key: String,
        base_url: String,
        exchanges_dir: PathBuf,
        concurrency: usize,
        inter_request_delay: Duration,
        redact_api_key: bool,
        verbose: bool,
    ) -> Self {
        Self {
            http,
            api_key,
            base_url,
            exchanges_dir,
            exchange_counter: AtomicUsize::new(0),
            redact_api_key,
            semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
            inter_request_delay,
            verbose,
            max_retries: 8,
            initial_delay: Duration::from_secs(3),
        }
    }

    /// Perform a GET request to a Splitwise endpoint with automatic retry on 429/5xx,
    /// recording the full exchange and returning the parsed JSON value.
    pub async fn get_json(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<serde_json::Value> {
        match self.get_json_optional(endpoint, query).await? {
            Some(v) => Ok(v),
            None => anyhow::bail!("Endpoint GET {} returned 404 Not Found", endpoint),
        }
    }

    /// Perform a GET request, returning `Ok(None)` on 404/403 errors (while still recording
    /// the exchange) and retrying on 429 / Cloudflare rate limits.
    pub async fn get_json_optional(
        &self,
        endpoint: &str,
        query: &[(&str, &str)],
    ) -> anyhow::Result<Option<serde_json::Value>> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .context("Semaphore acquisition failed")?;

        let url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );

        let mut attempts = 0u32;

        loop {
            if !self.inter_request_delay.is_zero() {
                tokio::time::sleep(self.inter_request_delay).await;
            }

            let timestamp = jiff::Zoned::now().to_string();
            let start = Instant::now();

            let req_builder = self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Accept", "application/json")
                .header("User-Agent", "splitwise_backup/0.4.0")
                .query(query);

            let res = match req_builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    if attempts < self.max_retries {
                        attempts += 1;
                        let delay = self.initial_delay * 2_u32.pow(attempts - 1);
                        eprintln! {
                            "  {STYLE_WARNING}⏳ Network error on {endpoint} ({e}) — retrying in {:.1}s ({attempts}/{max})...{STYLE_WARNING:#}",
                            delay.as_secs_f64(),
                            max = self.max_retries,
                        };
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e)
                        .with_context(|| format!("HTTP GET request failed for {endpoint}"));
                }
            };

            let duration_ms = start.elapsed().as_millis() as u64;
            let status = res.status();
            let status_code = status.as_u16();
            let status_text = status.canonical_reason().unwrap_or("").to_string();
            let resp_headers = header_map_to_btreemap(res.headers());

            // Handle rate limiting (429 or Cloudflare 1015)
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                || status_code == 503
                || status_code == 529
            {
                if attempts < self.max_retries {
                    attempts += 1;
                    let delay = if let Some(retry_after) =
                        res.headers().get(reqwest::header::RETRY_AFTER)
                    {
                        retry_after
                            .to_str()
                            .ok()
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(Duration::from_secs)
                            .unwrap_or(self.initial_delay * 2_u32.pow(attempts - 1))
                    } else {
                        // Exponential backoff: 3s, 6s, 12s, 24s, 48s, 60s max
                        let base = (self.initial_delay.as_secs() * 2_u64.pow(attempts - 1)).min(60);
                        Duration::from_secs(base)
                    };

                    eprintln! {
                        "  {STYLE_WARNING}⏳ Rate-limited (HTTP {status_code}) on {endpoint} — backing off {:.0}s before retry ({attempts}/{max})...{STYLE_WARNING:#}",
                        delay.as_secs_f64(),
                        max = self.max_retries,
                    };
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }

            let body_bytes = res
                .bytes()
                .await
                .with_context(|| format!("Failed reading response bytes from GET {endpoint}"))?;
            let byte_size = body_bytes.len();

            let json_body: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                Ok(v) => v,
                Err(_) => {
                    let text = String::from_utf8_lossy(&body_bytes);
                    serde_json::json!({
                        "_raw_text": text.as_ref()
                    })
                }
            };

            // Check if body contains Cloudflare rate limit error code 1015 even if status wasn't 429
            if let Some(text) = json_body.get("_raw_text").and_then(|t| t.as_str()) {
                if text.contains("1015") || text.contains("error code: 1015") {
                    if attempts < self.max_retries {
                        attempts += 1;
                        let delay = Duration::from_secs((5 * attempts as u64).min(60));
                        eprintln! {
                            "  {STYLE_WARNING}⏳ Cloudflare Rate Limit 1015 on {endpoint} — backing off {:.0}s before retry ({attempts}/{max})...{STYLE_WARNING:#}",
                            delay.as_secs_f64(),
                            max = self.max_retries,
                        };
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                }
            }

            // Construct query map
            let mut query_map = BTreeMap::new();
            for (k, v) in query {
                query_map.insert(k.to_string(), v.to_string());
            }

            // Construct request headers map
            let mut req_headers = BTreeMap::new();
            req_headers.insert("accept".to_string(), "application/json".to_string());
            req_headers.insert(
                "user-agent".to_string(),
                "splitwise_backup/0.4.0".to_string(),
            );
            let auth_val = if self.redact_api_key {
                "Bearer [REDACTED]".to_string()
            } else {
                format!("Bearer {}", self.api_key)
            };
            req_headers.insert("authorization".to_string(), auth_val);

            let exchange_index = self.exchange_counter.fetch_add(1, Ordering::SeqCst) + 1;
            let sanitized_endpoint = sanitize_name_for_exchange(endpoint);
            let exchange_id = format!("{exchange_index:04}_GET_{sanitized_endpoint}");

            let exchange = HttpExchange {
                exchange_id: exchange_id.clone(),
                request: HttpRequestRecord {
                    method: "GET".to_string(),
                    url: url.clone(),
                    path: format!("/api/v3.0/{}", endpoint.trim_start_matches('/')),
                    query: query_map,
                    headers: req_headers,
                },
                response: HttpResponseRecord {
                    status: status_code,
                    status_text,
                    headers: resp_headers,
                    body: json_body.clone(),
                },
                metadata: ExchangeMetadata {
                    timestamp,
                    duration_ms,
                    byte_size,
                },
            };

            // Write exchange file
            let exchange_path = self.exchanges_dir.join(format!("{exchange_id}.json"));
            let _ = write_json_file(&exchange_path, &serde_json::to_value(&exchange)?);

            if self.verbose {
                println! {
                    "  {STYLE_DIM}[{exchange_index:04}] GET {endpoint} → {} ({} ms, {} B){STYLE_DIM:#}",
                    status_code,
                    duration_ms,
                    byte_size
                };
            }

            // If 404 or 403, return Ok(None) so caller can gracefully skip deleted or inaccessible entities
            if status_code == 404 || status_code == 403 {
                return Ok(None);
            }

            if !status.is_success() {
                let preview = serde_json::to_string(&json_body).unwrap_or_default();
                anyhow::bail!(
                    "Splitwise API error on GET {} (HTTP {}): {}",
                    endpoint,
                    status_code,
                    preview
                );
            }

            return Ok(Some(json_body));
        }
    }

    /// Download media bytes from an arbitrary URL (CDN/S3/Splitwise) with retries.
    pub async fn download_media_bytes(
        &self,
        url: &str,
    ) -> anyhow::Result<(Vec<u8>, Option<String>)> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .context("Semaphore acquisition failed")?;

        let mut attempts = 0u32;
        loop {
            let mut req = self
                .http
                .get(url)
                .header("User-Agent", "splitwise_backup/0.4.0");

            if url.contains("splitwise.com") {
                req = req.header("Authorization", format!("Bearer {}", self.api_key));
            }

            let res = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if attempts < 3 {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                    return Err(e).with_context(|| format!("Media download failed for {url}"));
                }
            };

            let status = res.status();
            let status_code = status.as_u16();

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status_code == 503 || status_code == 529 {
                if attempts < self.max_retries {
                    attempts += 1;
                    let delay = if let Some(retry_after) =
                        res.headers().get(reqwest::header::RETRY_AFTER)
                    {
                        retry_after
                            .to_str()
                            .ok()
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(Duration::from_secs)
                            .unwrap_or(self.initial_delay * 2_u32.pow(attempts - 1))
                    } else {
                        let base = (self.initial_delay.as_secs() * 2_u64.pow(attempts - 1)).min(60);
                        Duration::from_secs(base)
                    };

                    eprintln! {
                        "  {STYLE_WARNING}⏳ Rate-limited on media {url} — backing off {:.0}s before retry ({attempts}/{max})...{STYLE_WARNING:#}",
                        delay.as_secs_f64(),
                        max = self.max_retries,
                    };
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }

            if !status.is_success() {
                anyhow::bail!("Media download HTTP status {}", status);
            }

            let content_type = res
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let bytes = res.bytes().await?.to_vec();
            return Ok((bytes, content_type));
        }
    }

    /// Get total number of HTTP exchanges logged so far.
    pub fn exchange_count(&self) -> usize {
        self.exchange_counter.load(Ordering::SeqCst)
    }
}

fn header_map_to_btreemap(headers: &HeaderMap) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (name, val) in headers.iter() {
        if let Ok(v) = val.to_str() {
            map.insert(name.as_str().to_ascii_lowercase(), v.to_string());
        }
    }
    map
}

fn sanitize_name_for_exchange(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | '?' | '&' | '=' | ':' | ' ' => '_',
            _ => c,
        })
        .collect()
}

pub fn write_json_file(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("Failed to create file: {}", path.display()))?;
    let writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(writer, value)
        .with_context(|| format!("Failed to write JSON to {}", path.display()))?;
    Ok(())
}

pub fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    if path.is_file() {
        if let Ok(file) = std::fs::File::open(path) {
            let reader = std::io::BufReader::new(file);
            return serde_json::from_reader(reader).ok();
        }
    }
    None
}
