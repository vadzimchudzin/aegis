//! Cloud decision support through OpenRouter, restricted to the Anthropic model family.

use std::time::{Duration, Instant};

use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::observer::{parse_decision, ModelDecision};

pub const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
pub const OPENROUTER_API: &str = "https://openrouter.ai/api/v1";
/// Default model; any `anthropic/*` model listed by OpenRouter can be selected instead.
pub const ANTHROPIC_MODEL: &str = "anthropic/claude-fable-5";
const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 32 * 1024;
const MAX_REQUEST_BYTES: usize = 36 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_TIMEOUT_SECS: u64 = 60;
const MAX_COMPLETION_TOKENS: u32 = 512;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudSettings {
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_model() -> String {
    ANTHROPIC_MODEL.into()
}

impl Default for CloudSettings {
    fn default() -> Self {
        Self { model: default_model() }
    }
}

impl CloudSettings {
    pub fn validate(&self) -> Result<(), String> {
        validate_model(&self.model)
    }
}

/// Accepts OpenRouter ids of the Anthropic family (`anthropic/claude-…`). Batch variants
/// answer asynchronously and cannot serve a trading decision, so they are refused.
pub fn validate_model(model: &str) -> Result<(), String> {
    let name = model.strip_prefix("anthropic/").unwrap_or("");
    let valid = !name.is_empty()
        && model.len() <= 100
        && !name.ends_with(":batch")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_' | b':'));
    if valid {
        Ok(())
    } else {
        Err("Only real-time OpenRouter models of the Anthropic family (anthropic/…) are supported".into())
    }
}

/// OpenRouter API base; `AEGIS_OPENROUTER_URL` points tests at a local stand-in.
pub fn api_base() -> String {
    std::env::var("AEGIS_OPENROUTER_URL")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| OPENROUTER_API.into())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_length: u64,
    /// USD per million prompt tokens.
    pub prompt_per_million: Option<f64>,
    /// USD per million completion tokens.
    pub completion_per_million: Option<f64>,
    /// The model accepts `response_format: json_object`, which AEGIS requires.
    pub json_mode: bool,
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(timeout.min(Duration::from_secs(5)))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "failed to initialize OpenRouter HTTP client".to_string())
}

/// Parses the OpenRouter `/models` catalog into Anthropic real-time models, newest first.
pub fn anthropic_models(catalog: &Value) -> Vec<ModelInfo> {
    let price = |value: &Value| {
        value
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| value.as_f64())
            .filter(|p| p.is_finite() && *p >= 0.0)
            .map(|p| (p * 1_000_000.0 * 1000.0).round() / 1000.0)
    };
    let mut models: Vec<ModelInfo> = catalog["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            validate_model(&id).ok()?;
            Some(ModelInfo {
                name: m["name"].as_str().unwrap_or(&id).to_string(),
                context_length: m["context_length"].as_u64().unwrap_or(0),
                prompt_per_million: price(&m["pricing"]["prompt"]),
                completion_per_million: price(&m["pricing"]["completion"]),
                json_mode: m["supported_parameters"]
                    .as_array()
                    .is_some_and(|p| p.iter().any(|x| x == "response_format")),
                id,
            })
        })
        .collect();
    // OpenRouter lists newer models first; keep that order but put JSON-capable ones first.
    models.sort_by_key(|m| !m.json_mode);
    models
}

/// Free catalog request (no key, not billed).
pub async fn list_models() -> Result<Vec<ModelInfo>, String> {
    let client = http_client(Duration::from_secs(15))?;
    let response = client
        .get(format!("{}/models", api_base()))
        .send()
        .await
        .map_err(|_| "OpenRouter model catalog is unreachable".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "OpenRouter catalog returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "failed to read the OpenRouter catalog".to_string())?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err("OpenRouter catalog exceeds the size limit".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "OpenRouter catalog is not JSON".to_string())?;
    let models = anthropic_models(&value);
    if models.is_empty() {
        return Err("OpenRouter catalog lists no Anthropic models".into());
    }
    Ok(models)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct KeyInfo {
    pub label: String,
    pub limit: Option<f64>,
    pub usage: Option<f64>,
}

/// Validates the key with OpenRouter's free key-info endpoint (no completion is billed).
pub async fn check_key(key: &str) -> Result<KeyInfo, String> {
    if key.trim().is_empty() {
        return Err("OpenRouter API key is required".into());
    }
    let client = http_client(Duration::from_secs(10))?;
    let response = client
        .get(format!("{}/key", api_base()))
        .bearer_auth(key.trim())
        .send()
        .await
        .map_err(|_| "OpenRouter is unreachable".to_string())?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err("OpenRouter rejected the API key (401/403): create a new key".into()),
        code => return Err(format!("OpenRouter key check returned HTTP {code}")),
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "OpenRouter key check returned invalid JSON".to_string())?;
    let data = &value["data"];
    Ok(KeyInfo {
        label: data["label"].as_str().unwrap_or("").chars().take(40).collect(),
        limit: data["limit"].as_f64(),
        usage: data["usage"].as_f64(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAnswer {
    pub response: String,
    pub latency_ms: u64,
    pub model: String,
}

/// Request one bounded, JSON-formatted answer. The API key is sent only as a
/// bearer header; request failures deliberately omit provider response bodies.
pub async fn consult(key: &str, prompt: &str, system: &str, timeout_secs: u64) -> Result<ProviderAnswer, String> {
    consult_model(key, ANTHROPIC_MODEL, prompt, system, timeout_secs).await
}

/// Same as [`consult`] with a selected Anthropic model.
pub async fn consult_model(
    key: &str,
    model: &str,
    prompt: &str,
    system: &str,
    timeout_secs: u64,
) -> Result<ProviderAnswer, String> {
    let endpoint = match std::env::var("AEGIS_OPENROUTER_URL") {
        Ok(_) => format!("{}/chat/completions", api_base()),
        Err(_) => OPENROUTER_CHAT_URL.into(),
    };
    consult_at_model(&endpoint, key, model, prompt, system, timeout_secs).await
}

#[cfg(test)]
async fn consult_at(
    endpoint: &str,
    key: &str,
    prompt: &str,
    system: &str,
    timeout_secs: u64,
) -> Result<ProviderAnswer, String> {
    consult_at_model(endpoint, key, ANTHROPIC_MODEL, prompt, system, timeout_secs).await
}

async fn consult_at_model(
    endpoint: &str,
    key: &str,
    model: &str,
    prompt: &str,
    system: &str,
    timeout_secs: u64,
) -> Result<ProviderAnswer, String> {
    validate_model(model)?;
    if key.trim().is_empty() {
        return Err("OpenRouter API key is required".into());
    }
    if prompt.trim().is_empty() || system.trim().is_empty() {
        return Err("prompt and system prompt must not be empty".into());
    }
    if prompt.len().saturating_add(system.len()) > MAX_PROMPT_BYTES {
        return Err("combined prompt exceeds the 32768-byte limit".into());
    }
    if !(1..=MAX_TIMEOUT_SECS).contains(&timeout_secs) {
        return Err(format!("timeout must be between 1 and {MAX_TIMEOUT_SECS} seconds"));
    }

    let body = serde_json::to_vec(&json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0,
        "max_tokens": MAX_COMPLETION_TOKENS,
        "response_format": {"type": "json_object"}
    }))
    .map_err(|_| "failed to encode OpenRouter request".to_string())?;
    if body.len() > MAX_REQUEST_BYTES {
        return Err("OpenRouter request exceeds the size limit".into());
    }

    let client = http_client(Duration::from_secs(timeout_secs))?;
    let started = Instant::now();
    let mut response = client
        .post(endpoint)
        .header(header::CONTENT_TYPE, "application/json")
        .header("HTTP-Referer", "https://aegis.local")
        .header("X-Title", "AEGIS")
        .bearer_auth(key.trim())
        .body(body)
        .send()
        .await
        .map_err(|_| "OpenRouter request failed or timed out".to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("OpenRouter returned HTTP {}", status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("OpenRouter response exceeds the 65536-byte limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "failed to read OpenRouter response".to_string())?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("OpenRouter response exceeds the 65536-byte limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| "OpenRouter returned invalid JSON".to_string())?;
    if body.get("error").is_some_and(|error| !error.is_null()) {
        return Err("OpenRouter returned an error envelope; no action accepted".into());
    }
    let finish_reason = body.pointer("/choices/0/finish_reason").and_then(Value::as_str);
    if finish_reason == Some("length") {
        return Err("OpenRouter response was truncated".into());
    }
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| "OpenRouter response has no decision content".to_string())?;
    if content.len() > MAX_RESPONSE_BYTES {
        return Err("OpenRouter decision content exceeds the size limit".into());
    }
    if finish_reason != Some("stop") {
        return Err("OpenRouter response did not finish successfully; no action accepted".into());
    }
    Ok(ProviderAnswer {
        response: content.to_owned(),
        latency_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        model: model.into(),
    })
}

/// Convenience wrapper for callers consuming the observer decision schema.
pub async fn cloud_decision(key: &str, prompt: &str, system: &str) -> Result<ModelDecision, String> {
    let answer = consult(key, prompt, system, 25).await?;
    parse_decision(&answer.response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn settings_default_and_restrict_to_anthropic_family() {
        assert_eq!(CloudSettings::default().model, "anthropic/claude-fable-5");
        assert!(CloudSettings::default().validate().is_ok());
        for ok in [
            "anthropic/claude-sonnet-4.5",
            "anthropic/claude-opus-5.5",
            "anthropic/claude-3.5-haiku:beta",
        ] {
            assert!(validate_model(ok).is_ok(), "{ok}");
        }
        for bad in [
            "openai/gpt-5",
            "anthropic/",
            "anthropic/claude-opus-5:batch",
            "anthropic/Claude",
            "anthropic/claude opus",
            "xanthropic/claude",
        ] {
            assert!(validate_model(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn catalog_keeps_only_realtime_anthropic_models() {
        let catalog = json!({"data": [
            {"id": "anthropic/claude-sonnet-5.5", "name": "Anthropic: Claude Sonnet 5.5", "context_length": 1000000,
             "pricing": {"prompt": "0.000002", "completion": "0.00001"}, "supported_parameters": ["response_format", "tools"]},
            {"id": "anthropic/claude-sonnet-5.5:batch", "name": "batch", "pricing": {}},
            {"id": "anthropic/claude-opus-4.1", "name": "Opus 4.1", "pricing": {"prompt": "0.000015"}, "supported_parameters": []},
            {"id": "openai/gpt-5", "name": "GPT"}
        ]});
        let models = anthropic_models(&catalog);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["anthropic/claude-sonnet-5.5", "anthropic/claude-opus-4.1"]
        );
        assert_eq!(models[0].prompt_per_million, Some(2.0));
        assert_eq!(models[0].completion_per_million, Some(10.0));
        assert!(models[0].json_mode);
        assert!(!models[1].json_mode);
    }

    #[tokio::test]
    async fn request_uses_fixed_model_json_and_header_only_secret() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(text.to_ascii_lowercase().contains("authorization: bearer test-secret"));
            let (_, body) = text.split_once("\r\n\r\n").unwrap();
            let body: Value = serde_json::from_str(body.trim()).unwrap();
            assert_eq!(body["model"], ANTHROPIC_MODEL);
            assert_eq!(body["max_tokens"], MAX_COMPLETION_TOKENS);
            assert_eq!(body["temperature"], 0);
            assert_eq!(body["response_format"]["type"], "json_object");
            let response = r#"{"choices":[{"finish_reason":"stop","message":{"content":"{\"ready\":true}"}}]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        });
        let answer = consult_at(
            &format!("http://{address}/chat"),
            "test-secret",
            "Return JSON",
            "JSON only",
            5,
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(answer.response, r#"{"ready":true}"#);
        assert_eq!(answer.model, ANTHROPIC_MODEL);
    }

    #[tokio::test]
    async fn rejects_invalid_call_parameters_without_network_access() {
        assert!(consult(" ", "x", "y", 5).await.is_err());
        assert!(consult("key", "x", "y", 0).await.is_err());
        assert!(consult("key", &"x".repeat(MAX_PROMPT_BYTES), "y", 5).await.is_err());
    }
}

#[cfg(test)]
#[path = "ai_provider_audit_tests.rs"]
mod audit_tests;

#[cfg(test)]
pub(crate) async fn audit_consult_at(
    endpoint: &str,
    prompt: &str,
    system: &str,
    timeout_secs: u64,
) -> Result<ProviderAnswer, String> {
    assert!(endpoint.starts_with("http://127.0.0.1:"));
    consult_at(endpoint, "fixture-key", prompt, system, timeout_secs).await
}
