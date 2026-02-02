use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub compact: bool,
    #[serde(default)]
    pub no_cost: bool,
    #[serde(default)]
    pub no_color: bool,
    #[serde(default)]
    pub breakdown: bool,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub cost: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        Self::load_internal(false)
    }

    pub fn load_quiet() -> Self {
        Self::load_internal(true)
    }

    fn load_internal(quiet: bool) -> Self {
        // Try config locations in order of priority
        let config_paths = Self::get_config_paths();

        for path in config_paths {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
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
