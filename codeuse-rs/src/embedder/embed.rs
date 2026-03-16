use crate::embedder::ollama_client::OllamaClient;
use crate::embedder::types::{Embedder, ModelInfo};
use crate::error::CodeuseError;

const BATCH_SIZE: usize = 32;

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

        for i in (0..inputs.len()).step_by(BATCH_SIZE) {
            let end = (i + BATCH_SIZE).min(inputs.len());
            let batch_refs: Vec<&str> = inputs[i..end].iter().map(|s| s.as_str()).collect();

            let embeddings = self.client.embed(&self.model_name, &batch_refs)?;
            results.extend(embeddings);

            if let Some(cb) = on_progress {
                cb(end, inputs.len());
            }
        }

        Ok(results)
    }

    fn embed_query(&self, input: &str) -> Result<Vec<f32>, CodeuseError> {
        let prefixed = format!("search_query: {input}");
        let embeddings = self.client.embed(&self.model_name, &[&prefixed])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| CodeuseError::Ollama("No embedding returned for query".into()))
    }
}
