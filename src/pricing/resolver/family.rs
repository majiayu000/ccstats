/// Allowlisted `LiteLLM` pricing families. Unknown families are skipped on ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PricingFamily {
    Anthropic,
    OpenAI,
    Xai,
    Google,
    DeepSeek,
    Qwen,
    Glm,
    Moonshot,
}

impl PricingFamily {
    pub(super) fn from_litellm_name(name: &str) -> Option<Self> {
        if name.contains("claude") {
            Some(Self::Anthropic)
        } else if name.starts_with("openai/")
            || name.starts_with("gpt-")
            || name.starts_with("codex")
        {
            Some(Self::OpenAI)
        } else if name.starts_with("xai/") || name.starts_with("grok-") {
            Some(Self::Xai)
        } else if name.starts_with("google/") || name.contains("gemini") {
            Some(Self::Google)
        } else if name.contains("deepseek") {
            Some(Self::DeepSeek)
        } else if name.contains("qwen") {
            Some(Self::Qwen)
        } else if name.contains("glm") {
            Some(Self::Glm)
        } else if name.starts_with("moonshot/") || name.contains("kimi") {
            Some(Self::Moonshot)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PricingFamily;

    #[test]
    fn google_matches_prefix_and_bare_gemini() {
        assert_eq!(
            PricingFamily::from_litellm_name("google/gemini-2.5-pro"),
            Some(PricingFamily::Google)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("gemini-2.5-flash"),
            Some(PricingFamily::Google)
        );
    }

    #[test]
    fn unknown_families_are_skipped() {
        assert_eq!(PricingFamily::from_litellm_name("mistral/large"), None);
        assert_eq!(PricingFamily::from_litellm_name("meta-llama/llama-3"), None);
    }

    #[test]
    fn existing_families_still_match() {
        assert_eq!(
            PricingFamily::from_litellm_name("claude-sonnet-4"),
            Some(PricingFamily::Anthropic)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("openai/gpt-4o"),
            Some(PricingFamily::OpenAI)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("xai/grok-4.3"),
            Some(PricingFamily::Xai)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("deepseek/deepseek-chat"),
            Some(PricingFamily::DeepSeek)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("dashscope/qwen-max"),
            Some(PricingFamily::Qwen)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("zai/glm-5"),
            Some(PricingFamily::Glm)
        );
        assert_eq!(
            PricingFamily::from_litellm_name("moonshot/moonshot-v1"),
            Some(PricingFamily::Moonshot)
        );
    }
}
