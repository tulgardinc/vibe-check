use crate::embedder::embed::OllamaEmbedder;
use crate::embedder::types::{
    ModelInfo, OllamaConfig, DEFAULT_MAX_INPUT_BYTES, max_input_bytes_from_context,
};
use crate::error::VibecheckError;
use crate::util::logger;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::thread;
use std::time::Duration;

const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_POLL_ATTEMPTS: u32 = 20;
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(500);
const EMBED_MAX_RETRIES: usize = 3;
const EMBED_RETRY_DELAY: Duration = Duration::from_millis(500);

/// Model detection priority: (base_name, default_tier).
/// Checked in order; first match wins.
const MODEL_PRIORITIES: &[(&str, &str)] = &[
    ("nomic-embed-code", "7b"),
    ("nomic-embed-text", "7b"),
];

/// Default query prefix for Nomic embedding models.
const DEFAULT_NOMIC_QUERY_PREFIX: &str = "search_query: ";

pub struct OllamaClient {
    client: Client,
    base_url: String,
}

#[derive(Deserialize)]
struct Model {
    name: String,
}

#[derive(Deserialize)]
struct ListResponse {
    models: Vec<Model>,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[derive(Serialize)]
struct ShowRequest<'a> {
    name: &'a str,
}

#[derive(Deserialize)]
struct ShowResponse {
    #[serde(default)]
    model_info: HashMap<String, serde_json::Value>,
}

/// Metadata extracted from Ollama's `/api/show` GGUF model_info.
#[derive(Debug, Default)]
pub struct OllamaModelMeta {
    pub context_length: Option<usize>,
    pub embedding_length: Option<usize>,
}

fn format_available_models(names: &[String]) -> String {
    if names.is_empty() {
        "\n  No models are currently installed in Ollama.".to_string()
    } else {
        format!("\n  Models currently in Ollama: {}", names.join(", "))
    }
}

impl OllamaClient {
    pub fn new(host: Option<&str>) -> Result<Self, VibecheckError> {
        let base_url = host
            .map(String::from)
            .or_else(|| std::env::var("OLLAMA_HOST").ok())
            .unwrap_or_else(|| DEFAULT_OLLAMA_HOST.to_string());

        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()?;

        Ok(Self { client, base_url })
    }

    pub fn check_health(&self) -> bool {
        self.list_models().is_ok()
    }

    pub fn list_models(&self) -> Result<Vec<String>, VibecheckError> {
        let resp: ListResponse = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()?
            .json()?;
        Ok(resp.models.into_iter().map(|m| m.name).collect())
    }

    pub fn embed(&self, model: &str, inputs: &[&str]) -> Result<Vec<Vec<f32>>, VibecheckError> {
        let mut last_err = None;
        for attempt in 0..EMBED_MAX_RETRIES {
            if attempt > 0 {
                logger::verbose(&format!(
                    "Retrying embed request (attempt {}/{})",
                    attempt + 1,
                    EMBED_MAX_RETRIES
                ));
                thread::sleep(EMBED_RETRY_DELAY);
            }

            let send_result = self
                .client
                .post(format!("{}/api/embed", self.base_url))
                .json(&EmbedRequest { model, input: inputs })
                .send();

            let response = match send_result {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };

            let status = response.status();
            let body = response.text()?;

            if !status.is_success() {
                return Err(VibecheckError::Ollama(format!(
                    "Ollama embed returned {status}: {body}"
                )));
            }

            let resp: EmbedResponse = serde_json::from_str(&body).map_err(|e| {
                VibecheckError::Ollama(format!(
                    "Failed to parse Ollama response: {e}\nResponse body (first 500 chars): {}",
                    &body[..body.len().min(500)]
                ))
            })?;

            return Ok(resp.embeddings);
        }

        Err(last_err.unwrap().into())
    }

    /// Resolve which model to use. Checks (in order): explicit argument, VIBECHECK_MODEL env var, auto-detect.
    pub fn resolve_model(&self, config: &OllamaConfig) -> Result<ModelInfo, VibecheckError> {
        let model_name = config
            .model
            .clone()
            .or_else(|| std::env::var("VIBECHECK_MODEL").ok());

        match model_name {
            Some(name) => {
                // Verify the model exists in Ollama
                let names = self.list_models()?;
                let found = names.iter().any(|n| n == &name || n.starts_with(&format!("{name}:")));
                if !found {
                    let available = format_available_models(&names);
                    return Err(VibecheckError::Ollama(format!(
                        "Model '{name}' not found in Ollama.{available}\n\n\
                         Pull it with: ollama pull {name}"
                    )));
                }
                let meta = self.show_model(&name);
                let dimensions = self.resolve_dimensions(config, &meta, &name)?;
                let max_input_bytes = Self::resolve_max_input_bytes(config, &meta);
                let query_prefix = Self::resolve_query_prefix(config, &name);
                Ok(ModelInfo {
                    name,
                    dimensions,
                    tier: "custom".to_string(),
                    max_input_bytes,
                    query_prefix,
                })
            }
            None => self.detect_model(config),
        }
    }

    fn detect_model(&self, config: &OllamaConfig) -> Result<ModelInfo, VibecheckError> {
        let names = self.list_models()?;

        for (base_name, default_tier) in MODEL_PRIORITIES {
            // Exact match or :latest tag
            if names.iter().any(|n| n == *base_name || n == &format!("{base_name}:latest")) {
                let meta = self.show_model(base_name);
                let dimensions = self.resolve_dimensions(config, &meta, base_name)?;
                let max_input_bytes = Self::resolve_max_input_bytes(config, &meta);
                let query_prefix = Self::resolve_query_prefix(config, base_name);
                return Ok(ModelInfo {
                    name: base_name.to_string(),
                    dimensions,
                    tier: default_tier.to_string(),
                    max_input_bytes,
                    query_prefix,
                });
            }
            // Tagged variant (e.g., nomic-embed-code:137m)
            if let Some(found) = names.iter().find(|n| n.starts_with(&format!("{base_name}:"))) {
                let meta = self.show_model(found);
                let dimensions = self.resolve_dimensions(config, &meta, found)?;
                let tier = found.split(':').nth(1).unwrap_or("custom");
                let max_input_bytes = Self::resolve_max_input_bytes(config, &meta);
                let query_prefix = Self::resolve_query_prefix(config, found);
                return Ok(ModelInfo {
                    name: found.clone(),
                    dimensions,
                    tier: tier.to_string(),
                    max_input_bytes,
                    query_prefix,
                });
            }
        }

        let available = format_available_models(&names);

        Err(VibecheckError::Ollama(format!(
            "No compatible embedding model found in Ollama.{available}\n\n\
             vibecheck needs a Nomic embedding model. Choose one:\n\n\
             \x20   ollama pull nomic-embed-text      (recommended, 274 MB, works on CPU and GPU)\n\
             \x20   ollama pull nomic-embed-code      (code-specific, if available)\n\n\
             Or specify any Ollama embedding model with --model or VIBECHECK_MODEL env var.\n\
             After pulling, re-run this command."
        )))
    }

    fn detect_dimensions(&self, model_name: &str) -> Result<usize, VibecheckError> {
        let embeddings = self.embed(model_name, &["test"])?;
        match embeddings.first() {
            Some(v) => Ok(v.len()),
            None => Err(VibecheckError::Ollama(
                "No embedding returned for dimension detection".into(),
            )),
        }
    }

    /// Query Ollama's `/api/show` for GGUF model metadata.
    /// Returns defaults on any failure — never fatal.
    fn show_model(&self, model_name: &str) -> OllamaModelMeta {
        let resp = self
            .client
            .post(format!("{}/api/show", self.base_url))
            .json(&ShowRequest { name: model_name })
            .send();

        let body: ShowResponse = match resp {
            Ok(r) if r.status().is_success() => match r.json() {
                Ok(parsed) => parsed,
                Err(e) => {
                    logger::verbose(&format!("Failed to parse /api/show response: {e}"));
                    return OllamaModelMeta::default();
                }
            },
            Ok(r) => {
                logger::verbose(&format!("/api/show returned {}", r.status()));
                return OllamaModelMeta::default();
            }
            Err(e) => {
                logger::verbose(&format!("/api/show request failed: {e}"));
                return OllamaModelMeta::default();
            }
        };

        let arch = body
            .model_info
            .get("general.architecture")
            .and_then(|v| v.as_str())
            .map(String::from);

        let arch = match arch {
            Some(a) => a,
            None => return OllamaModelMeta::default(),
        };

        let context_length = body
            .model_info
            .get(&format!("{arch}.context_length"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let embedding_length = body
            .model_info
            .get(&format!("{arch}.embedding_length"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        OllamaModelMeta {
            context_length,
            embedding_length,
        }
    }

    /// Resolve dimensions from overrides, env vars, metadata, or probe embedding.
    fn resolve_dimensions(
        &self,
        config: &OllamaConfig,
        meta: &OllamaModelMeta,
        model_name: &str,
    ) -> Result<usize, VibecheckError> {
        // Priority: CLI dimensions > env DIMENSIONS > metadata > probe
        if let Some(v) = config.dimensions {
            return Ok(v);
        }
        if let Some(v) = std::env::var("VIBECHECK_DIMENSIONS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        {
            return Ok(v);
        }
        if let Some(v) = meta.embedding_length {
            return Ok(v);
        }
        self.detect_dimensions(model_name)
    }

    /// Resolve query prefix from overrides, env vars, or model-name heuristic.
    fn resolve_query_prefix(config: &OllamaConfig, model_name: &str) -> String {
        // Priority: CLI query_prefix > env QUERY_PREFIX > auto-detect from model name
        if let Some(ref v) = config.query_prefix {
            return v.clone();
        }
        if let Ok(v) = std::env::var("VIBECHECK_QUERY_PREFIX") {
            return v;
        }
        // Nomic models use "search_query: " prefix; others get no prefix
        if model_name.starts_with("nomic-embed") {
            DEFAULT_NOMIC_QUERY_PREFIX.to_string()
        } else {
            String::new()
        }
    }

    /// Compute max_input_bytes from overrides, env vars, and model metadata.
    fn resolve_max_input_bytes(config: &OllamaConfig, meta: &OllamaModelMeta) -> usize {
        // Priority: CLI max_input_bytes > env MAX_INPUT_BYTES > CLI context_length > env CONTEXT_LENGTH > metadata > default
        if let Some(v) = config.max_input_bytes {
            return v;
        }
        if let Some(v) = std::env::var("VIBECHECK_MAX_INPUT_BYTES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        {
            return v;
        }
        if let Some(v) = config.context_length {
            return max_input_bytes_from_context(v);
        }
        if let Some(v) = std::env::var("VIBECHECK_CONTEXT_LENGTH")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        {
            return max_input_bytes_from_context(v);
        }
        if let Some(v) = meta.context_length {
            return max_input_bytes_from_context(v);
        }
        DEFAULT_MAX_INPUT_BYTES
    }

    /// Try to start Ollama as a background process.
    ///
    /// The spawned child process is intentionally not waited on — `ollama serve` is a
    /// long-running server that should outlive vibecheck.  The child handle is dropped,
    /// which detaches the process on Unix.
    pub fn try_start_ollama(&self) -> bool {
        let result = Command::new("ollama")
            .args(["serve"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(_detached_child) => {
                logger::info("Ollama not running — starting it automatically...");

                for _ in 0..STARTUP_POLL_ATTEMPTS {
                    thread::sleep(STARTUP_POLL_INTERVAL);
                    if self.check_health() {
                        logger::info("Ollama started.");
                        return true;
                    }
                }
                false
            }
            Err(_) => false,
        }
    }

    pub fn preflight(self, config: &OllamaConfig) -> Result<(OllamaEmbedder, String), VibecheckError> {
        let mut healthy = self.check_health();

        if !healthy && self.try_start_ollama() {
            healthy = self.check_health();
        }

        if !healthy {
            return Err(VibecheckError::Ollama(
                "Ollama is not running and could not be started automatically.\n\n\
                 To set up Ollama:\n\
                 \x20 1. Install from https://ollama.com\n\
                 \x20 2. Run: ollama serve\n\
                 \x20 3. Pull a model: ollama pull nomic-embed-text\n\
                 \x20 4. Re-run this command."
                    .into(),
            ));
        }

        let model = self.resolve_model(config)?;
        let message = format!(
            "Ollama is running. Using {} ({}, {}d, {}b max input).",
            model.name, model.tier, model.dimensions, model.max_input_bytes
        );

        let embedder = OllamaEmbedder::new(self, &model);

        Ok((embedder, message))
    }
}
