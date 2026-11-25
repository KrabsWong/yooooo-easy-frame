use std::{env, fs, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Chinese,
    English,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub language: Language,
    pub automatic_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Chinese,
            automatic_updates: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let Ok(text) = fs::read_to_string(settings_path()) else {
            return Self::default();
        };
        let language = if text.contains("\"language\"")
            && text.to_ascii_lowercase().contains("english")
        {
            Language::English
        } else {
            Language::Chinese
        };
        Self {
            language,
            automatic_updates: parse_bool(&text, "automatic_updates").unwrap_or(true),
        }
    }

    pub fn save(self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let content = format!(
            "{{\n  \"language\": \"{}\",\n  \"automatic_updates\": {}\n}}\n",
            match self.language {
                Language::Chinese => "zh-CN",
                Language::English => "en",
            },
            self.automatic_updates
        );
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, content).is_ok() {
            let _ = fs::rename(temporary, path);
        }
    }
}

fn parse_bool(text: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let line = text
        .lines()
        .find(|line| line.contains(&needle) && line.contains(':'))?;
    let value = line
        .split_once(':')?
        .1
        .trim()
        .trim_end_matches([',', '}'])
        .trim();
    match value.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn settings_path() -> PathBuf {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library")
        .join("Application Support")
        .join("Easy Frame")
        .join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_chinese() {
        assert_eq!(Settings::default().language, Language::Chinese);
    }

    #[test]
    fn automatic_updates_default_to_enabled() {
        assert!(Settings::default().automatic_updates);
    }

    #[test]
    fn parses_persisted_automatic_updates() {
        assert_eq!(
            parse_bool("{\"automatic_updates\": false}", "automatic_updates"),
            Some(false)
        );
        assert_eq!(
            parse_bool("{\"automatic_updates\": true}", "automatic_updates"),
            Some(true)
        );
        assert_eq!(parse_bool("{}", "automatic_updates"), None);
    }
}
