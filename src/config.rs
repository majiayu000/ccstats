use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConfigSortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConfigColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConfigCostMode {
    Show,
    Hide,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) offline: bool,
    #[serde(default)]
    pub(crate) compact: bool,
    #[serde(default)]
    pub(crate) no_cost: bool,
    #[serde(default)]
    pub(crate) no_color: bool,
    #[serde(default)]
    pub(crate) breakdown: bool,
    #[serde(default)]
    pub(crate) debug: bool,
    #[serde(default)]
    pub(crate) strict_pricing: bool,
    #[serde(default)]
    pub(crate) order: Option<ConfigSortOrder>,
    #[serde(default)]
    pub(crate) color: Option<ConfigColorMode>,
    #[serde(default)]
    pub(crate) cost: Option<ConfigCostMode>,
    #[serde(default)]
    pub(crate) timezone: Option<String>,
    #[serde(default)]
    pub(crate) locale: Option<String>,
}

impl Config {
    pub(crate) fn load() -> Self {
        Self::load_internal(false)
    }

    pub(crate) fn load_quiet() -> Self {
        Self::load_internal(true)
    }

    fn load_internal(quiet: bool) -> Self {
        Self::load_from_paths(&Self::get_config_paths(), quiet)
    }

    fn load_from_paths(paths: &[PathBuf], quiet: bool) -> Self {
        for path in paths {
            if path.exists()
                && let Ok(content) = fs::read_to_string(path)
            {
                match toml::from_str::<Config>(&content) {
                    Ok(config) => {
                        if !quiet {
                            eprintln!("Loaded config from {}", path.display());
                        }
                        return config;
                    }
                    Err(e) => {
                        if !quiet {
                            eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        Self::default()
    }

    fn get_config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // 1. XDG config: ~/.config/ccstats/config.toml (Linux/cross-platform)
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".config").join("ccstats").join("config.toml"));
        }

        // 2. macOS Application Support: ~/Library/Application Support/ccstats/config.toml
        if let Some(config_dir) = dirs::config_dir() {
            let macos_path = config_dir.join("ccstats").join("config.toml");
            if !paths.contains(&macos_path) {
                paths.push(macos_path);
            }
        }

        // 3. Home directory: ~/.ccstats.toml
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".ccstats.toml"));
        }

        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_paths_non_empty() {
        let paths = Config::get_config_paths();
        assert!(!paths.is_empty());
    }

    #[test]
    fn test_config_paths_contain_expected_filenames() {
        let paths = Config::get_config_paths();
        let has_xdg = paths.iter().any(|p| {
            p.to_string_lossy()
                .contains(".config/ccstats/config.toml")
        });
        let has_dotfile = paths
            .iter()
            .any(|p| p.to_string_lossy().ends_with(".ccstats.toml"));
        assert!(has_xdg);
        assert!(has_dotfile);
    }

    // --- TOML deserialization tests ---

    #[test]
    fn test_deserialize_empty_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert!(!config.offline);
        assert!(!config.compact);
        assert!(!config.no_cost);
        assert!(!config.no_color);
        assert!(!config.breakdown);
        assert!(!config.debug);
        assert!(!config.strict_pricing);
        assert!(config.order.is_none());
        assert!(config.color.is_none());
        assert!(config.cost.is_none());
        assert!(config.timezone.is_none());
        assert!(config.locale.is_none());
    }

    #[test]
    fn test_deserialize_all_booleans_true() {
        let toml_str = r#"
offline = true
compact = true
no_cost = true
no_color = true
breakdown = true
debug = true
strict_pricing = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.offline);
        assert!(config.compact);
        assert!(config.no_cost);
        assert!(config.no_color);
        assert!(config.breakdown);
        assert!(config.debug);
        assert!(config.strict_pricing);
    }

    #[test]
    fn test_deserialize_partial_booleans() {
        let config: Config = toml::from_str("compact = true\ndebug = true").unwrap();
        assert!(config.compact);
        assert!(config.debug);
        assert!(!config.offline);
        assert!(!config.breakdown);
    }

    #[test]
    fn test_deserialize_sort_order_asc() {
        let config: Config = toml::from_str("order = \"asc\"").unwrap();
        assert!(matches!(config.order, Some(ConfigSortOrder::Asc)));
    }

    #[test]
    fn test_deserialize_sort_order_desc() {
        let config: Config = toml::from_str("order = \"desc\"").unwrap();
        assert!(matches!(config.order, Some(ConfigSortOrder::Desc)));
    }

    #[test]
    fn test_deserialize_color_modes() {
        for (input, expected) in [
            ("auto", "auto"),
            ("always", "always"),
            ("never", "never"),
        ] {
            let config: Config =
                toml::from_str(&format!("color = \"{}\"", input)).unwrap();
            match config.color {
                Some(ConfigColorMode::Auto) => assert_eq!(expected, "auto"),
                Some(ConfigColorMode::Always) => assert_eq!(expected, "always"),
                Some(ConfigColorMode::Never) => assert_eq!(expected, "never"),
                None => panic!("expected Some"),
            }
        }
    }

    #[test]
    fn test_deserialize_cost_modes() {
        let show: Config = toml::from_str("cost = \"show\"").unwrap();
        assert!(matches!(show.cost, Some(ConfigCostMode::Show)));

        let hide: Config = toml::from_str("cost = \"hide\"").unwrap();
        assert!(matches!(hide.cost, Some(ConfigCostMode::Hide)));
    }

    #[test]
    fn test_deserialize_string_fields() {
        let config: Config = toml::from_str(
            "timezone = \"Asia/Tokyo\"\nlocale = \"ja-JP\"",
        )
        .unwrap();
        assert_eq!(config.timezone.as_deref(), Some("Asia/Tokyo"));
        assert_eq!(config.locale.as_deref(), Some("ja-JP"));
    }

    #[test]
    fn test_deserialize_full_config() {
        let toml_str = r#"
offline = true
compact = false
no_cost = true
no_color = false
breakdown = true
debug = false
strict_pricing = true
order = "desc"
color = "never"
cost = "hide"
timezone = "US/Eastern"
locale = "en-US"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.offline);
        assert!(!config.compact);
        assert!(config.no_cost);
        assert!(!config.no_color);
        assert!(config.breakdown);
        assert!(!config.debug);
        assert!(config.strict_pricing);
        assert!(matches!(config.order, Some(ConfigSortOrder::Desc)));
        assert!(matches!(config.color, Some(ConfigColorMode::Never)));
        assert!(matches!(config.cost, Some(ConfigCostMode::Hide)));
        assert_eq!(config.timezone.as_deref(), Some("US/Eastern"));
        assert_eq!(config.locale.as_deref(), Some("en-US"));
    }

    #[test]
    fn test_deserialize_invalid_enum_value() {
        let result = toml::from_str::<Config>("order = \"random\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_wrong_type() {
        let result = toml::from_str::<Config>("offline = \"yes\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_enum_case_sensitive() {
        // enums use rename_all = "lowercase", so uppercase should fail
        let result = toml::from_str::<Config>("order = \"Asc\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_unknown_fields_ignored() {
        // serde default behavior: unknown fields cause error unless deny_unknown_fields
        // Config doesn't have deny_unknown_fields, so unknown fields should be ignored
        let result = toml::from_str::<Config>("unknown_field = true");
        // TOML + serde without deny_unknown_fields ignores unknown keys
        assert!(result.is_ok());
    }

    // --- load_from_paths tests ---

    fn write_temp_config(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_load_from_valid_file() {
        let f = write_temp_config("compact = true\noffline = true");
        let config =
            Config::load_from_paths(&[f.path().to_path_buf()], true);
        assert!(config.compact);
        assert!(config.offline);
    }

    #[test]
    fn test_load_from_nonexistent_path_returns_default() {
        let config = Config::load_from_paths(
            &[PathBuf::from("/nonexistent/path/config.toml")],
            true,
        );
        assert!(!config.offline);
        assert!(!config.compact);
    }

    #[test]
    fn test_load_from_empty_paths_returns_default() {
        let config = Config::load_from_paths(&[], true);
        assert!(!config.offline);
        assert!(config.order.is_none());
    }

    #[test]
    fn test_load_priority_first_valid_wins() {
        let f1 = write_temp_config("compact = true");
        let f2 = write_temp_config("compact = false\noffline = true");
        let config = Config::load_from_paths(
            &[f1.path().to_path_buf(), f2.path().to_path_buf()],
            true,
        );
        // First file wins
        assert!(config.compact);
        // Second file's offline=true is NOT loaded
        assert!(!config.offline);
    }

    #[test]
    fn test_load_skips_invalid_toml_tries_next() {
        let bad = write_temp_config("this is not valid toml [[[");
        let good = write_temp_config("debug = true");
        let config = Config::load_from_paths(
            &[bad.path().to_path_buf(), good.path().to_path_buf()],
            true,
        );
        assert!(config.debug);
    }

    #[test]
    fn test_load_skips_nonexistent_tries_next() {
        let good = write_temp_config("breakdown = true");
        let config = Config::load_from_paths(
            &[
                PathBuf::from("/no/such/file.toml"),
                good.path().to_path_buf(),
            ],
            true,
        );
        assert!(config.breakdown);
    }

    #[test]
    fn test_load_all_invalid_returns_default() {
        let bad1 = write_temp_config("not valid [[[");
        let bad2 = write_temp_config("also bad {{{");
        let config = Config::load_from_paths(
            &[bad1.path().to_path_buf(), bad2.path().to_path_buf()],
            true,
        );
        assert!(!config.offline);
        assert!(!config.compact);
    }

    // --- Default trait ---

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.offline);
        assert!(!config.compact);
        assert!(!config.no_cost);
        assert!(!config.no_color);
        assert!(!config.breakdown);
        assert!(!config.debug);
        assert!(!config.strict_pricing);
        assert!(config.order.is_none());
        assert!(config.color.is_none());
        assert!(config.cost.is_none());
        assert!(config.timezone.is_none());
        assert!(config.locale.is_none());
    }
}
