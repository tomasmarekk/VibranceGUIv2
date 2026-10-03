//! Validated settings shared by IPC, persistence, and the foreground observer.
//! Profiles default to executable-name matching to preserve the original workflow.

use crate::platform::DisplayMode;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SettingsError {
    #[error("invalid settings: {0}")]
    Invalid(String),
    #[error("failed to access settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to decode settings: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ColorSettings {
    pub vibrance: f64,
    pub brightness: f64,
    pub gamma: f64,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            vibrance: 50.0,
            brightness: 50.0,
            gamma: 1.0,
        }
    }
}

impl ColorSettings {
    pub(crate) fn validate(self) -> Result<(), SettingsError> {
        for (name, value, minimum, maximum) in [
            ("vibrance", self.vibrance, 0.0, 100.0),
            ("brightness", self.brightness, 0.0, 100.0),
            ("gamma", self.gamma, 0.5, 3.0),
        ] {
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(SettingsError::Invalid(format!(
                    "{name} must be between {minimum} and {maximum}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub(crate) struct GlobalSettings {
    pub autostart: bool,
    pub primary_only: bool,
    pub never_change_resolution: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Profile {
    pub id: String,
    pub name: String,
    pub executable_path: String,
    #[serde(default)]
    pub match_by_path: bool,
    pub color: ColorSettings,
    pub resolution: Option<DisplayMode>,
}

impl Profile {
    pub(crate) fn validate(&self) -> Result<(), SettingsError> {
        if self.id.is_empty() || self.id.len() > 128 || self.id.chars().any(char::is_control) {
            return Err(SettingsError::Invalid(
                "profile ID is empty or invalid".into(),
            ));
        }
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
        {
            return Err(SettingsError::Invalid(
                "program name is empty or invalid".into(),
            ));
        }
        validate_executable(&self.executable_path)?;
        self.color.validate()?;
        if let Some(mode) = &self.resolution
            && (mode.width < 320
                || mode.width > 16384
                || mode.height < 200
                || mode.height > 16384
                || mode.refresh_rate < 1
                || mode.refresh_rate > 1000)
        {
            return Err(SettingsError::Invalid(
                "resolution is outside the supported range".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn matches(&self, executable_name: &str, executable_path: &str) -> bool {
        if self.match_by_path {
            normalized_path(&self.executable_path) == normalized_path(executable_path)
        } else {
            executable_stem(&self.executable_path)
                .eq_ignore_ascii_case(&executable_stem(executable_name))
        }
    }
}

pub(crate) fn executable_stem(value: &str) -> String {
    let name = value.rsplit(['/', '\\']).next().unwrap_or(value);
    let stem = name
        .get(..name.len().saturating_sub(4))
        .filter(|_| {
            name.get(name.len().saturating_sub(4)..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
        })
        .unwrap_or(name);
    stem.to_owned()
}

fn normalized_path(value: &str) -> String {
    value.replace('/', "\\").to_lowercase()
}

pub(crate) fn validate_executable(value: &str) -> Result<(), SettingsError> {
    let absolute = Path::new(value).is_absolute()
        || (value.as_bytes().get(1) == Some(&b':')
            && value
                .as_bytes()
                .get(2)
                .is_some_and(|b| *b == b'\\' || *b == b'/'))
        || value.starts_with("\\\\");
    if value.len() > 32768
        || value.chars().any(char::is_control)
        || !absolute
        || !value.to_lowercase().ends_with(".exe")
    {
        return Err(SettingsError::Invalid(
            "select an absolute path to a Windows .exe file".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Config {
    pub schema_version: u32,
    pub settings: GlobalSettings,
    pub desktop: ColorSettings,
    pub profiles: Vec<Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            settings: GlobalSettings::default(),
            desktop: ColorSettings::default(),
            profiles: Vec::new(),
        }
    }
}

impl Config {
    pub(crate) fn validate(&self) -> Result<(), SettingsError> {
        if self.schema_version != 1 {
            return Err(SettingsError::Invalid(
                "unsupported configuration version".into(),
            ));
        }
        if self.profiles.len() > 512 {
            return Err(SettingsError::Invalid(
                "at most 512 profiles are supported".into(),
            ));
        }
        self.desktop.validate()?;
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for profile in &self.profiles {
            profile.validate()?;
            if !ids.insert(&profile.id) || !paths.insert(normalized_path(&profile.executable_path))
            {
                return Err(SettingsError::Invalid("duplicate program profile".into()));
            }
        }
        Ok(())
    }

    pub(crate) fn matching_profile(&self, name: &str, path: &str) -> Option<&Profile> {
        // Explicit path matches take precedence over legacy basename matches.
        self.profiles
            .iter()
            .filter(|p| p.match_by_path)
            .chain(self.profiles.iter().filter(|p| !p.match_by_path))
            .find(|p| p.matches(name, path))
    }
}

pub(crate) struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub(crate) fn load(&self) -> Result<Config, SettingsError> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BYTES {
            return Err(SettingsError::Invalid("configuration exceeds 2 MiB".into()));
        }
        let config: Config = serde_json::from_slice(&bytes)?;
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn save(&self, config: &Config) -> Result<(), SettingsError> {
        config.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| SettingsError::Invalid("settings path has no parent".into()))?;
        fs::create_dir_all(parent)?;
        // Preserve an unreadable previous config before an explicit user save replaces it.
        if self.path.exists() && self.load().is_err() {
            let mut target = tempfile::Builder::new()
                .prefix("settings.invalid-")
                .suffix(".json")
                .tempfile_in(parent)?;
            std::io::copy(&mut fs::File::open(&self.path)?, &mut target)?;
            target.as_file().sync_all()?;
            target
                .keep()
                .map_err(|error| SettingsError::Io(error.error))?;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, config)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .map_err(|error| SettingsError::Io(error.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(path: &str) -> Profile {
        Profile {
            id: path.into(),
            name: "Game".into(),
            executable_path: path.into(),
            match_by_path: false,
            color: ColorSettings::default(),
            resolution: None,
        }
    }

    #[test]
    fn legacy_matching_uses_case_insensitive_executable_stem() {
        let game = profile("C:\\Games\\Game.EXE");
        assert!(game.matches("game", "D:\\Other\\game.exe"));
        assert!(game.matches("GAME.exe", ""));
        assert!(!game.matches("GameLauncher.exe", ""));
    }

    #[test]
    fn exact_path_match_wins_over_legacy_match() {
        let mut exact = profile("D:\\Other\\game.exe");
        exact.match_by_path = true;
        let config = Config {
            profiles: vec![profile("C:\\Games\\game.exe"), exact.clone()],
            ..Config::default()
        };
        assert_eq!(
            config.matching_profile("game", "d:/other/GAME.exe"),
            Some(&exact)
        );
    }

    #[test]
    fn color_boundary_rejects_nonfinite_and_out_of_range_values() {
        for value in [f64::NAN, f64::INFINITY, -0.1, 100.1] {
            assert!(
                ColorSettings {
                    vibrance: value,
                    ..ColorSettings::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            ColorSettings {
                gamma: 0.49,
                ..ColorSettings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ColorSettings {
                gamma: 3.0,
                brightness: 0.0,
                vibrance: 100.0
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn duplicate_program_paths_and_unknown_schema_are_rejected() {
        let config = Config {
            profiles: vec![profile("C:\\game.exe"), profile("c:/GAME.exe")],
            ..Config::default()
        };
        assert!(config.validate().is_err());
        assert!(
            Config {
                schema_version: 2,
                ..Config::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn settings_roundtrip_preserves_profiles_for_missing_executables() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("settings.json"));
        assert_eq!(store.load().unwrap(), Config::default());
        let config = Config {
            profiles: vec![profile("Z:\\Missing\\game.exe")],
            ..Config::default()
        };
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap(), config);
        store.save(&Config::default()).unwrap();
        assert_eq!(store.load().unwrap(), Config::default());
    }

    #[test]
    fn corrupted_settings_are_preserved_before_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"broken data").unwrap();
        let store = ConfigStore::new(path.clone());
        assert!(store.load().is_err());
        store.save(&Config::default()).unwrap();
        fs::write(&path, b"another broken file").unwrap();
        store.save(&Config::default()).unwrap();
        let mut backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("settings.invalid-")
            })
            .map(|path| fs::read(path).unwrap())
            .collect();
        backups.sort();
        assert_eq!(
            backups,
            [b"another broken file".to_vec(), b"broken data".to_vec()]
        );
    }
}
