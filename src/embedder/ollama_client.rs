use crate::embedder::embed::OllamaEmbedder;
use crate::embedder::types::ModelInfo;
use crate::error::VibecheckError;
use crate::util::logger;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::thread;
use std::time::Duration;

const DEFAULT_OLLAMA_HOST: &str = "http://localhost:11434";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_POLL_ATTEMPTS: u32 = 20;
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(500);

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

fn format_available_models(names: &[String]) -> String {
    if names.is_empty() {
        "\n  No models are currently installed in Ollama.".to_string()
    } else {
        format!("\n  Models currently in Ollama: {}", names.join(", "))
    }
}

impl OllamaClient {
    pub fn new(host: Option<&str>) -> Self {
        let base_url = host
            .map(String::from)
            .or_else(|| std::env::var("OLLAMA_HOST").ok())
            .unwrap_or_else(|| DEFAULT_OLLAMA_HOST.to_string());

        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("failed to build HTTP client");

        Self { client, base_url }
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
        let response = self
            .client
            .post(format!("{}/api/embed", self.base_url))
            .json(&EmbedRequest { model, input: inputs })
            .send()?;

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

        Ok(resp.embeddings)
    }

    /// Resolve which model to use. Checks (in order): explicit argument, VIBECHECK_MODEL env var, auto-detect.
    pub fn resolve_model(&self, explicit: Option<&str>) -> Result<ModelInfo, VibecheckError> {
        let model_name = explicit
            .map(String::from)
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
                let dimensions = self.detect_dimensions(&name)?;
                Ok(ModelInfo {
                    name,
                    dimensions,
                    tier: "custom".to_string(),
                })
            }
            None => self.detect_model(),
        }
    }

    fn detect_model(&self) -> Result<ModelInfo, VibecheckError> {
        let names = self.list_models()?;

        struct Candidate {
            matches: fn(&str) -> bool,
            resolve: fn(&str) -> String,
            tier: &'static str,
        }

        let candidates: Vec<Candidate> = vec![
            Candidate {
                matches: |n| n == "nomic-embed-code:latest" || n == "nomic-embed-code",
                resolve: |_| "nomic-embed-code".into(),
                tier: "7b",
            },
            Candidate {
                matches: |n| n.starts_with("nomic-embed-code:137m"),
                resolve: |n| n.to_string(),
                tier: "137m",
            },
            Candidate {
                matches: |n| n == "nomic-embed-text:latest" || n == "nomic-embed-text",
                resolve: |_| "nomic-embed-text".into(),
                tier: "7b",
            },
            Candidate {
                matches: |n| n.starts_with("nomic-embed-text:"),
                resolve: |n| n.to_string(),
                tier: "137m",
            },
        ];

        for candidate in &candidates {
            if let Some(found) = names.iter().find(|n| (candidate.matches)(n)) {
                let model_name = (candidate.resolve)(found);
                let dimensions = self.detect_dimensions(&model_name)?;
                return Ok(ModelInfo {
                    name: model_name,
                    dimensions,
                    tier: candidate.tier.to_string(),
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

    pub fn try_start_ollama(&self) -> bool {
        let result = Command::new("ollama")
            .args(["serve"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(_child) => {
                // Child is detached by not calling .wait()
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

    pub fn preflight(self, model_override: Option<&str>) -> Result<(OllamaEmbedder, String), VibecheckError> {
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

        let model = self.resolve_model(model_override)?;
        let message = format!(
            "Ollama is running. Using {} ({}, {}d).",
            model.name, model.tier, model.dimensions
        );

        let embedder = OllamaEmbedder::new(self, &model);

        Ok((embedder, message))
    }
}
