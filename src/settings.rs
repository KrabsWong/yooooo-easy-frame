use std::{env, fs, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Chinese,
    English,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub language: Language,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::Chinese,
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
        Self { language }
    }

    pub fn save(self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let content = format!(
            "{{\n  \"language\": \"{}\"\n}}\n",
            match self.language {
                Language::Chinese => "zh-CN",
                Language::English => "en",
            }
        );
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, content).is_ok() {
            let _ = fs::rename(temporary, path);
        }
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
}
