/// Model pricing info (per token, not per million)
#[derive(Debug, Clone, Default)]
pub(super) struct ModelPricing {
    pub(super) input: f64,
    pub(super) output: f64,
    pub(super) reasoning_output: f64,
    pub(super) cache_read: f64,
    pub(super) cache_create: f64,
}
