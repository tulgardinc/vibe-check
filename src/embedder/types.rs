use crate::error::VibecheckError;
use crate::util::logger;

/// Fallback truncation limit when model metadata is unavailable.
/// Conservative default based on typical 8K-token embedding model contexts at ~2 chars/token.
pub const DEFAULT_MAX_INPUT_BYTES: usize = 16_000;

/// Conservative chars-per-token estimate for code.
pub const CHARS_PER_TOKEN_ESTIMATE: usize = 2;

pub fn max_input_bytes_from_context(context_length: usize) -> usize {
    context_length * CHARS_PER_TOKEN_ESTIMATE
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub dimensions: usize,
    pub tier: String,
    pub max_input_bytes: usize,
    pub query_prefix: String,
}

#[derive(Debug, Clone, Default)]
pub struct OllamaConfig {
    pub model: Option<String>,
    pub host: Option<String>,
    pub context_length: Option<usize>,
    pub max_input_bytes: Option<usize>,
    pub query_prefix: Option<String>,
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
            let (embedder, msg) = client.preflight(config)?;
            logger::info(&msg);
            Ok((Box::new(embedder), msg))
        }
    }
}
