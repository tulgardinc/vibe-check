use crate::error::VibecheckError;
use crate::util::logger;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub dimensions: usize,
    pub tier: String,
}

#[derive(Debug, Clone, Default)]
pub struct OllamaConfig {
    pub model: Option<String>,
    pub host: Option<String>,
}

pub trait Embedder {
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn tier(&self) -> &str;
    fn embed_batch(
        &self,
        inputs: &[&str],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<Vec<f32>>, VibecheckError>;
    fn embed_query(&self, input: &str) -> Result<Vec<f32>, VibecheckError>;
}

/// Resolve an embedder: use the injected one if provided, otherwise create one via Ollama preflight.
pub fn resolve_embedder(
    injected: Option<Box<dyn Embedder>>,
    config: &OllamaConfig,
) -> Result<(Box<dyn Embedder>, String), VibecheckError> {
    match injected {
        Some(e) => {
            let msg = format!("Using injected embedder: {}", e.model_name());
            Ok((e, msg))
        }
        None => {
            let client = crate::embedder::ollama_client::OllamaClient::new(config.host.as_deref())?;
            let (embedder, msg) = client.preflight(config.model.as_deref())?;
            logger::info(&msg);
            Ok((Box::new(embedder), msg))
        }
    }
}
