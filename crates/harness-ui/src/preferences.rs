use crate::theme::{Accent, Backdrop};
use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

const PREFERENCES_VERSION: u8 = 6;
const PREFERENCES_FILE: &str = "gpui-settings.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FontPreference {
    #[default]
    Geist,
    System,
    Humanist,
    Rounded,
    Serif,
    Mono,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct NativePreferences {
    version: u8,
    pub(crate) theme: ThemePreference,
    pub(crate) font: FontPreference,
    pub(crate) accent: Accent,
    pub(crate) backdrop: Backdrop,
    pub(crate) sidebar_glass: u8,
    pub(crate) rail_width: u16,
    pub(crate) session_order: HashMap<String, Vec<String>>,
    pub(crate) hidden_models: HashSet<String>,
    pub(crate) selected_model_key: Option<String>,
    pub(crate) model_by_source: HashMap<String, SourceSelection>,
}

impl Default for NativePreferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            theme: ThemePreference::System,
            font: FontPreference::Geist,
            accent: Accent::Neutral,
            backdrop: Backdrop::Default,
            sidebar_glass: 35,
            rail_width: 248,
            session_order: HashMap::new(),
            hidden_models: HashSet::new(),
            selected_model_key: None,
            model_by_source: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceSelection {
    pub(crate) model_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) service_tier: Option<String>,
}

impl NativePreferences {
    pub(crate) fn load() -> Result<Self> {
        let Some(path) = preferences_path() else {
            return Ok(Self::default());
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", path.display()));
            }
        };
        let mut preferences = serde_json::from_slice::<Self>(&bytes)
            .with_context(|| format!("parse {}", path.display()))?;
        if preferences.version < 4 {
            preferences.sidebar_glass = 35;
        }
        if preferences.version < 5 {
            preferences.rail_width = 248;
        }
        preferences.version = PREFERENCES_VERSION;
        preferences.sidebar_glass = preferences.sidebar_glass.min(60);
        preferences.rail_width = preferences.rail_width.clamp(177, 420);
        Ok(preferences)
    }

    pub(crate) fn save(&self) -> Result<()> {
        let path = preferences_path().context("the platform has no configuration directory")?;
        let parent = path.parent().context("preferences path has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        let bytes = serde_json::to_vec_pretty(self).context("encode native preferences")?;
        fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
    }

    pub(crate) fn reset_file() -> Result<()> {
        let Some(path) = preferences_path() else {
            return Ok(());
        };
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
        }
    }
}

fn preferences_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("Personal Harness").join(PREFERENCES_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_keep_stable_defaults() {
        let preferences: NativePreferences = serde_json::from_str("{}").unwrap();

        assert_eq!(preferences.theme, ThemePreference::System);
        assert_eq!(preferences.font, FontPreference::Geist);
        assert_eq!(preferences.accent, Accent::Neutral);
        assert_eq!(preferences.sidebar_glass, 35);
        assert_eq!(preferences.rail_width, 248);
        assert!(preferences.session_order.is_empty());
        assert!(preferences.hidden_models.is_empty());
        assert_eq!(preferences.selected_model_key, None);
        assert!(preferences.model_by_source.is_empty());
    }

    #[test]
    fn preferences_round_trip_without_model_order_dependence() {
        let mut preferences = NativePreferences {
            theme: ThemePreference::Dark,
            font: FontPreference::Mono,
            accent: Accent::Lavender,
            backdrop: Backdrop::Plum,
            sidebar_glass: 35,
            rail_width: 312,
            ..NativePreferences::default()
        };
        preferences.session_order.insert(
            "/work/harness".into(),
            vec!["thread-2".into(), "thread-1".into()],
        );
        preferences.hidden_models.insert("codex:gpt-5".into());
        preferences.selected_model_key = Some("cursor:composer-2".into());
        preferences.model_by_source.insert(
            "cursor".into(),
            SourceSelection {
                model_key: "cursor:composer-2".into(),
                effort: Some("high".into()),
                service_tier: Some("fast".into()),
            },
        );
        let json = serde_json::to_string(&preferences).unwrap();
        let decoded: NativePreferences = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, preferences);
    }
}
