use crate::error::CodeuseError;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub dimensions: usize,
    pub tier: String,
}

pub trait Embedder {
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn tier(&self) -> &str;
    fn embed_batch(
        &self,
        inputs: &[String],
        on_progress: Option<&dyn Fn(usize, usize)>,
    ) -> Result<Vec<Vec<f32>>, CodeuseError>;
    fn embed_query(&self, input: &str) -> Result<Vec<f32>, CodeuseError>;
}
