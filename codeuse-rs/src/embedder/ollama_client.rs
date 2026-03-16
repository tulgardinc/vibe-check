use crate::embedder::embed::OllamaEmbedder;
use crate::embedder::types::ModelInfo;
use crate::error::CodeuseError;
use crate::util::logger;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::thread;
use std::time::Duration;

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

impl OllamaClient {
    pub fn new(host: Option<&str>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            base_url: host.unwrap_or("http://localhost:11434").to_string(),
        }
    }

    pub fn check_health(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .is_ok()
    }

    pub fn list_models(&self) -> Result<Vec<String>, CodeuseError> {
        let resp: ListResponse = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()?
            .json()?;
        Ok(resp.models.into_iter().map(|m| m.name).collect())
    }

    pub fn embed(&self, model: &str, inputs: &[&str]) -> Result<Vec<Vec<f32>>, CodeuseError> {
        let resp: EmbedResponse = self
            .client
            .post(format!("{}/api/embed", self.base_url))
            .json(&EmbedRequest { model, input: inputs })
            .send()?
            .json()?;
        Ok(resp.embeddings)
    }

    pub fn detect_model(&self) -> Result<ModelInfo, CodeuseError> {
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

        let available = if names.is_empty() {
            "\n  No models are currently installed in Ollama.".to_string()
        } else {
            format!("\n  Models currently in Ollama: {}", names.join(", "))
        };

        Err(CodeuseError::Ollama(format!(
            "No compatible embedding model found in Ollama.{available}\n\n\
             vibecheck needs a Nomic embedding model. Choose one:\n\n\
             \x20   ollama pull nomic-embed-text      (recommended, 274 MB, works on CPU and GPU)\n\
             \x20   ollama pull nomic-embed-code      (code-specific, if available)\n\n\
             The model runs locally with no API keys or cloud dependencies.\n\
             After pulling, re-run this command."
        )))
    }

    fn detect_dimensions(&self, model_name: &str) -> Result<usize, CodeuseError> {
        let embeddings = self.embed(model_name, &["test"])?;
        match embeddings.first() {
            Some(v) => Ok(v.len()),
            None => Err(CodeuseError::Ollama(
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
            Ok(child) => {
                // Detach the child process
                let _ = child.id();
                logger::info("Ollama not running — starting it automatically...");

                for _ in 0..10 {
                    thread::sleep(Duration::from_millis(500));
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

    pub fn preflight(&self) -> Result<(OllamaEmbedder<'_>, String), CodeuseError> {
        let mut healthy = self.check_health();

        if !healthy {
            if self.try_start_ollama() {
                healthy = self.check_health();
            }
        }

        if !healthy {
            return Err(CodeuseError::Ollama(
                "Ollama is not running and could not be started automatically.\n\n\
                 To set up Ollama:\n\
                 \x20 1. Install from https://ollama.com\n\
                 \x20 2. Run: ollama serve\n\
                 \x20 3. Pull a model: ollama pull nomic-embed-text\n\
                 \x20 4. Re-run this command."
                    .into(),
            ));
        }

        let model = self.detect_model()?;
        let message = format!(
            "Ollama is running. Using {} ({}, {}d).",
            model.name, model.tier, model.dimensions
        );

        let embedder = OllamaEmbedder::new(self, &model);

        Ok((embedder, message))
    }
}
