use crate::embedder::ollama_client::OllamaClient;
use crate::embedder::types::{Embedder, ModelInfo};
use crate::error::CodeuseError;

/// Per-input character limit. nomic-embed-code has 8192 token context;
/// code tokenizes at roughly 2-3 chars/token, so 16K chars is conservative.
const MAX_INPUT_CHARS: usize = 16_000;

fn truncate_input(input: &str) -> &str {
    if input.len() <= MAX_INPUT_CHARS {
        input
    } else {
        let mut end = MAX_INPUT_CHARS;
        while end > 0 && !input.is_char_boundary(end) {
            end -= 1;
        }
        &input[..end]
    }
}

pub struct OllamaEmbedder<'a> {
    client: &'a OllamaClient,
    model_name: String,
    dimensions: usize,
    tier: String,
}

impl<'a> OllamaEmbedder<'a> {
    pub fn new(client: &'a OllamaClient, model: &ModelInfo) -> Self {
        Self {
            client,
            model_name: model.name.clone(),
            dimensions: model.dimensions,
            tier: model.tier.clone(),
        }
    }
}

impl Embedder for OllamaEmbedder<'_> {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn tier(&self) -> &str {
        &self.tier
    }

    fn embed_batch(
        &self,
        inputs: &[String],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<Vec<f32>>, CodeuseError> {
        let mut results = Vec::with_capacity(inputs.len());

        for (i, input) in inputs.iter().enumerate() {
            let truncated = truncate_input(input);
            let embeddings = self.client.embed(&self.model_name, &[truncated])?;
            results.extend(embeddings);

            if let Some(cb) = on_progress {
                cb(i + 1, inputs.len());
            }
        }

        Ok(results)
    }

    fn embed_query(&self, input: &str) -> Result<Vec<f32>, CodeuseError> {
        let prefixed = format!("search_query: {}", truncate_input(input));
        let embeddings = self.client.embed(&self.model_name, &[&prefixed])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| CodeuseError::Ollama("No embedding returned for query".into()))
    }
}
