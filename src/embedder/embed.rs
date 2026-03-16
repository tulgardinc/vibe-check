use crate::embedder::ollama_client::OllamaClient;
use crate::embedder::types::{Embedder, ModelInfo};
use crate::error::VibecheckError;
use crate::util::logger;

/// Per-input character limit. nomic-embed-code has 8192 token context;
/// code tokenizes at roughly 2-3 chars/token, so 16K chars is conservative.
const MAX_INPUT_BYTES: usize = 16_000;

/// Number of inputs to send per Ollama API call.
const EMBED_BATCH_SIZE: usize = 32;

const SEARCH_QUERY_PREFIX: &str = "search_query: ";

fn truncate_input<'a>(input: &'a str, context: &str) -> &'a str {
    if input.len() <= MAX_INPUT_BYTES {
        input
    } else {
        logger::warn(&format!(
            "Truncating input from {} to {MAX_INPUT_BYTES} bytes ({context})",
            input.len()
        ));
        let mut end = MAX_INPUT_BYTES;
        while end > 0 && !input.is_char_boundary(end) {
            end -= 1;
        }
        &input[..end]
    }
}

pub struct OllamaEmbedder {
    client: OllamaClient,
    model_name: String,
    dimensions: usize,
    tier: String,
}

impl OllamaEmbedder {
    pub fn new(client: OllamaClient, model: &ModelInfo) -> Self {
        Self {
            client,
            model_name: model.name.clone(),
            dimensions: model.dimensions,
            tier: model.tier.clone(),
        }
    }
}

impl Embedder for OllamaEmbedder {
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
    ) -> Result<Vec<Vec<f32>>, VibecheckError> {
        let mut results = Vec::with_capacity(inputs.len());

        for chunk in inputs.chunks(EMBED_BATCH_SIZE) {
            let truncated: Vec<&str> = chunk
                .iter()
                .map(|s| truncate_input(s, "batch"))
                .collect();
            let embeddings = self.client.embed(&self.model_name, &truncated)?;
            results.extend(embeddings);

            if let Some(cb) = on_progress {
                cb(results.len(), inputs.len());
            }
        }

        Ok(results)
    }

    fn embed_query(&self, input: &str) -> Result<Vec<f32>, VibecheckError> {
        let truncated = truncate_input(input, "query");
        let prefixed = format!("{SEARCH_QUERY_PREFIX}{truncated}");
        let embeddings = self.client.embed(&self.model_name, &[&prefixed])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| VibecheckError::Ollama("No embedding returned for query".into()))
    }
}
