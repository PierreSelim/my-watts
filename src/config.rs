use crate::GpsAnalyzerError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn env_or_dot(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| ".".to_string()))
}

fn default_moving_speed_threshold_kmh() -> f64 {
    3.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BikeConfig {
    pub name: String,
    /// Rolling resistance coefficient
    pub crr: f64,
    /// Drag area Cd*A in m²
    pub cda: f64,
    /// Speed threshold below which the rider is considered stopped (km/h)
    #[serde(default = "default_moving_speed_threshold_kmh")]
    pub moving_speed_threshold_kmh: f64,
}

fn default_rider_weight_kg() -> f64 {
    75.0
}

fn default_bike_name() -> String {
    "road".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub bikes: Vec<BikeConfig>,
    #[serde(default = "default_rider_weight_kg")]
    pub default_rider_weight_kg: f64,
    #[serde(default = "default_bike_name")]
    pub default_bike: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::builtin_defaults()
    }
}

pub fn my_watts_home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        env_or_dot("USERPROFILE").join(".my-watts")
    }
    #[cfg(not(target_os = "windows"))]
    {
        env_or_dot("HOME").join(".my-watts")
    }
}

pub fn analysis_dir() -> PathBuf {
    my_watts_home_dir().join("analysis")
}

pub fn index_path() -> PathBuf {
    my_watts_home_dir().join("index.json")
}

impl AppConfig {
    pub fn default_config_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            env_or_dot("APPDATA").join("my-watts").join("config.toml")
        }
        #[cfg(not(target_os = "windows"))]
        {
            env_or_dot("HOME")
                .join(".config")
                .join("my-watts")
                .join("config.toml")
        }
    }

    /// Ordered list of paths probed when no explicit `--config` is given.
    /// The first existing file wins. XDG/APPDATA has priority over the home-dir fallback.
    pub fn config_search_paths() -> Vec<PathBuf> {
        vec![
            Self::default_config_path(),
            my_watts_home_dir().join("config.toml"),
        ]
    }

    /// Load from an explicit path (must exist), or probe `config_search_paths` in order
    /// (falls back to built-in defaults if no file is found).
    pub fn load_or_default(config_path: Option<&Path>) -> Result<Self, GpsAnalyzerError> {
        match config_path {
            Some(path) => {
                let content = std::fs::read_to_string(path).map_err(GpsAnalyzerError::Io)?;
                let config: AppConfig = toml::from_str(&content)
                    .map_err(|e| GpsAnalyzerError::ConfigError(e.to_string()))?;
                Ok(Self::merge_with_builtin(config))
            }
            None => {
                for path in Self::config_search_paths() {
                    if path.exists() {
                        let content =
                            std::fs::read_to_string(&path).map_err(GpsAnalyzerError::Io)?;
                        let config: AppConfig = toml::from_str(&content)
                            .map_err(|e| GpsAnalyzerError::ConfigError(e.to_string()))?;
                        return Ok(Self::merge_with_builtin(config));
                    }
                }
                Ok(Self::builtin_defaults())
            }
        }
    }

    /// Append any built-in bikes whose name is not already present in `config.bikes`.
    /// This lets user configs override specific presets while still having access to the rest.
    fn merge_with_builtin(mut config: AppConfig) -> AppConfig {
        for bike in Self::builtin_defaults().bikes {
            if config.find_bike(&bike.name).is_none() {
                config.bikes.push(bike);
            }
        }
        config
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
                    moving_speed_threshold_kmh: 3.0,
                },
                BikeConfig {
                    name: "gravel".to_string(),
                    crr: 0.006,
                    cda: 0.40,
                    moving_speed_threshold_kmh: 3.0,
                },
                BikeConfig {
                    name: "mountain".to_string(),
                    crr: 0.012,
                    cda: 0.57,
                    moving_speed_threshold_kmh: 3.0,
                },
                BikeConfig {
                    name: "hybrid".to_string(),
                    crr: 0.008,
                    cda: 0.46,
                    moving_speed_threshold_kmh: 3.0,
                },
            ],
            default_rider_weight_kg: 75.0,
            default_bike: "road".to_string(),
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
        // User bike is present with correct values
        let user_bike = config
            .find_bike("test-gravel")
            .expect("user-defined bike should be present");
        assert_eq!(user_bike.crr, 0.006);
        assert_eq!(user_bike.moving_speed_threshold_kmh, 3.0);
        // Built-in presets are also available (merged in)
        assert!(config.find_bike("road").is_some());
        assert!(config.find_bike("gravel").is_some());
        assert!(config.find_bike("mountain").is_some());
    }

    #[test]
    fn test_moving_speed_threshold_default_is_3() {
        let config = AppConfig::builtin_defaults();
        for bike in &config.bikes {
            assert_eq!(
                bike.moving_speed_threshold_kmh, 3.0,
                "bike '{}' should have default threshold 3.0",
                bike.name
            );
        }
    }

    #[test]
    fn test_moving_speed_threshold_overridable_per_bike() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(
            temp,
            r#"
[[bikes]]
name = "gravel"
crr = 0.006
cda = 0.40
moving_speed_threshold_kmh = 5.0
"#
        )
        .unwrap();
        let config = AppConfig::load_or_default(Some(temp.path())).unwrap();
        assert_eq!(config.bikes[0].moving_speed_threshold_kmh, 5.0);
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

    #[test]
    fn test_default_rider_weight_is_75() {
        let config = AppConfig::builtin_defaults();
        assert_eq!(config.default_rider_weight_kg, 75.0);
    }

    #[test]
    fn test_default_bike_is_road() {
        let config = AppConfig::builtin_defaults();
        assert_eq!(config.default_bike, "road");
    }

    #[test]
    fn test_toml_override_rider_weight() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "default_rider_weight_kg = 80.0").unwrap();
        let config = AppConfig::load_or_default(Some(temp.path())).unwrap();
        assert_eq!(config.default_rider_weight_kg, 80.0);
        assert_eq!(config.default_bike, "road");
    }

    #[test]
    fn test_toml_missing_new_fields_uses_serde_defaults() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(
            temp,
            r#"
[[bikes]]
name = "test"
crr = 0.004
cda = 0.32
"#
        )
        .unwrap();
        let config = AppConfig::load_or_default(Some(temp.path())).unwrap();
        assert_eq!(config.default_rider_weight_kg, 75.0);
        assert_eq!(config.default_bike, "road");
    }

    #[test]
    fn test_default_impl_matches_builtin_defaults() {
        let via_default = AppConfig::default();
        let via_builtin = AppConfig::builtin_defaults();
        assert_eq!(
            via_default.default_rider_weight_kg,
            via_builtin.default_rider_weight_kg
        );
        assert_eq!(via_default.default_bike, via_builtin.default_bike);
        assert_eq!(via_default.bikes.len(), via_builtin.bikes.len());
    }

    #[test]
    fn test_config_search_paths_xdg_is_first() {
        let paths = AppConfig::config_search_paths();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], AppConfig::default_config_path());
        assert_eq!(paths[1], my_watts_home_dir().join("config.toml"));
    }

    #[test]
    fn test_load_or_default_uses_home_config_when_xdg_absent() {
        let temp_home = tempfile::tempdir().unwrap();
        let home_config = temp_home.path().join(".my-watts").join("config.toml");
        std::fs::create_dir_all(home_config.parent().unwrap()).unwrap();
        std::fs::write(&home_config, "default_rider_weight_kg = 90.0\n").unwrap();

        // Point HOME / USERPROFILE at temp dir so my_watts_home_dir() resolves there,
        // while APPDATA / XDG config stays absent (no file at that path).
        #[cfg(target_os = "windows")]
        std::env::set_var("USERPROFILE", temp_home.path());
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("HOME", temp_home.path());

        let config = AppConfig::load_or_default(None).unwrap();

        #[cfg(target_os = "windows")]
        std::env::remove_var("USERPROFILE");
        #[cfg(not(target_os = "windows"))]
        std::env::remove_var("HOME");

        assert_eq!(config.default_rider_weight_kg, 90.0);
    }

    #[test]
    fn test_my_watts_home_dir_ends_with_my_watts() {
        let dir = my_watts_home_dir();
        assert_eq!(dir.file_name().unwrap(), ".my-watts");
    }

    #[test]
    fn test_analysis_dir_ends_with_analysis() {
        let dir = analysis_dir();
        assert_eq!(dir.file_name().unwrap(), "analysis");
    }

    #[test]
    fn test_analysis_dir_is_inside_my_watts_home() {
        let home = my_watts_home_dir();
        let analysis = analysis_dir();
        assert_eq!(analysis.parent().unwrap(), home);
    }

    #[test]
    fn test_index_path_is_index_json_inside_my_watts_home() {
        let path = index_path();
        assert_eq!(path.file_name().unwrap(), "index.json");
        assert_eq!(path.parent().unwrap(), my_watts_home_dir());
    }
}
