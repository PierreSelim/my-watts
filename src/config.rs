use crate::GpsAnalyzerError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BikeConfig {
    pub name: String,
    /// Rolling resistance coefficient
    pub crr: f64,
    /// Drag area Cd*A in m²
    pub cda: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub bikes: Vec<BikeConfig>,
}

impl AppConfig {
    pub fn default_config_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(appdata).join("my-watts").join("config.toml")
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home)
                .join(".config")
                .join("my-watts")
                .join("config.toml")
        }
    }

    /// Load from an explicit path (must exist), or from the default path (falls back to built-in
    /// defaults if not found).
    pub fn load_or_default(config_path: Option<&Path>) -> Result<Self, GpsAnalyzerError> {
        match config_path {
            Some(path) => {
                let content = std::fs::read_to_string(path).map_err(GpsAnalyzerError::Io)?;
                toml::from_str(&content).map_err(|e| GpsAnalyzerError::ConfigError(e.to_string()))
            }
            None => {
                let default_path = Self::default_config_path();
                if default_path.exists() {
                    let content =
                        std::fs::read_to_string(&default_path).map_err(GpsAnalyzerError::Io)?;
                    toml::from_str(&content)
                        .map_err(|e| GpsAnalyzerError::ConfigError(e.to_string()))
                } else {
                    Ok(Self::builtin_defaults())
                }
            }
        }
    }

    pub fn find_bike(&self, name: &str) -> Option<&BikeConfig> {
        self.bikes
            .iter()
            .find(|b| b.name.eq_ignore_ascii_case(name))
    }

    fn builtin_defaults() -> Self {
        AppConfig {
            bikes: vec![
                BikeConfig {
                    name: "road".to_string(),
                    crr: 0.004,
                    cda: 0.32,
                },
                BikeConfig {
                    name: "gravel".to_string(),
                    crr: 0.006,
                    cda: 0.40,
                },
                BikeConfig {
                    name: "mountain".to_string(),
                    crr: 0.012,
                    cda: 0.57,
                },
                BikeConfig {
                    name: "hybrid".to_string(),
                    crr: 0.008,
                    cda: 0.46,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_builtin_defaults_includes_gravel() {
        let config = AppConfig::builtin_defaults();
        assert!(config.find_bike("gravel").is_some());
    }

    #[test]
    fn test_builtin_defaults_all_bikes_present() {
        let config = AppConfig::builtin_defaults();
        for name in &["road", "gravel", "mountain", "hybrid"] {
            assert!(config.find_bike(name).is_some(), "missing bike: {}", name);
        }
    }

    #[test]
    fn test_find_bike_case_insensitive() {
        let config = AppConfig::builtin_defaults();
        assert!(config.find_bike("Road").is_some());
        assert!(config.find_bike("GRAVEL").is_some());
    }

    #[test]
    fn test_find_bike_not_found() {
        let config = AppConfig::builtin_defaults();
        assert!(config.find_bike("unicycle").is_none());
    }

    #[test]
    fn test_load_from_toml() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(
            temp,
            r#"
[[bikes]]
name = "test-gravel"
crr = 0.006
cda = 0.38
"#
        )
        .unwrap();

        let config = AppConfig::load_or_default(Some(temp.path())).unwrap();
        assert_eq!(config.bikes.len(), 1);
        assert_eq!(config.bikes[0].name, "test-gravel");
        assert_eq!(config.bikes[0].crr, 0.006);
    }

    #[test]
    fn test_load_explicit_missing_file_errors() {
        let result = AppConfig::load_or_default(Some(Path::new("/nonexistent/path/config.toml")));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_no_path_returns_defaults_when_no_file() {
        // Force no config file by unsetting HOME/APPDATA (can't easily do that cross-platform,
        // but we can verify the function succeeds and returns bikes).
        // This test just checks the happy path: no explicit path → some bikes returned.
        let config = AppConfig::load_or_default(None).unwrap();
        assert!(!config.bikes.is_empty());
    }
}
