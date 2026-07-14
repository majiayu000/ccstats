/// Model pricing info (per token, not per million)
#[derive(Debug, Clone, Default)]
pub(super) struct ModelPricing {
    pub(super) input: f64,
    pub(super) output: f64,
    pub(super) reasoning_output: f64,
    pub(super) cache_read: f64,
    pub(super) cache_create: f64,
    /// Cache creation with 1-hour TTL (falls back to `cache_create` when the
    /// provider does not publish a separate 1h rate).
    pub(super) cache_create_1h: f64,
}

/// Normalize version separators: some hosters spell version dots as `p`
/// (e.g. Fireworks `glm-5p2` == `glm-5.2`). Convert a `p` sitting between two
/// digits into `.` so a dot-spelled gateway alias can match the hoster key.
pub(super) fn dot_version_variant(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == 'p'
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit()
        {
            out.push('.');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_version_variant_converts_p_between_digits() {
        assert_eq!(dot_version_variant("glm-5p2"), "glm-5.2");
        assert_eq!(dot_version_variant("glm-4p5"), "glm-4.5");
    }

    #[test]
    fn dot_version_variant_leaves_others_unchanged() {
        assert_eq!(dot_version_variant("glm-5"), "glm-5");
        assert_eq!(dot_version_variant("deepseek-chat"), "deepseek-chat");
        assert_eq!(dot_version_variant("ap-1"), "ap-1");
    }
}
