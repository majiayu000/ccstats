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
        // Try config locations in order of priority
        let config_paths = Self::get_config_paths();

        for path in config_paths {
            if path.exists()
                && let Ok(content) = fs::read_to_string(&path)
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

    #[test]
    fn test_config_paths() {
        let paths = Config::get_config_paths();
        for p in &paths {
            println!("Path: {:?}, exists: {}", p, p.exists());
        }
        assert!(!paths.is_empty());
    }
}
