//! Configuração do usuário (config.json). Campos ausentes ou arquivo ruim → padrões.

use crate::theme::{Period, period_for};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "dia")]
    Day,
    #[serde(rename = "por-do-sol")]
    Sunset,
    #[serde(rename = "noite")]
    Night,
}

impl ThemeMode {
    pub fn next(self) -> Self {
        match self {
            ThemeMode::Auto => ThemeMode::Day,
            ThemeMode::Day => ThemeMode::Sunset,
            ThemeMode::Sunset => ThemeMode::Night,
            ThemeMode::Night => ThemeMode::Auto,
        }
    }

    pub fn resolve(self, hour: u32, minute: u32) -> Period {
        match self {
            ThemeMode::Auto => period_for(hour, minute),
            ThemeMode::Day => Period::Day,
            ThemeMode::Sunset => Period::Sunset,
            ThemeMode::Night => Period::Night,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::Auto => "Auto",
            ThemeMode::Day => "Dia",
            ThemeMode::Sunset => "Pôr do sol",
            ThemeMode::Night => "Noite",
        }
    }
}

/// O que a pílula compacta mostra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompactMode {
    /// Música enquanto toca; senão, o relógio.
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "musica")]
    Music,
    #[serde(rename = "relogio")]
    Clock,
}

impl CompactMode {
    pub fn next(self) -> Self {
        match self {
            CompactMode::Auto => CompactMode::Music,
            CompactMode::Music => CompactMode::Clock,
            CompactMode::Clock => CompactMode::Auto,
        }
    }

    pub fn shows_music(self, has_track: bool, playing: bool) -> bool {
        match self {
            CompactMode::Auto => has_track && playing,
            CompactMode::Music => has_track,
            CompactMode::Clock => false,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CompactMode::Auto => "Pílula: automático",
            CompactMode::Music => "Pílula: música",
            CompactMode::Clock => "Pílula: relógio",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: String,
    pub theme: ThemeMode,
    pub spotify_client_id: String,
    /// Esconde a pílula quando o app em primeiro plano ocupa a tela inteira (jogos).
    pub hide_in_fullscreen: bool,
    pub compact: CompactMode,
    /// Avisos do WhatsApp na ilha.
    pub whatsapp: bool,
    /// Mostrar o texto das mensagens (desligado: só "Nova mensagem").
    pub whatsapp_preview: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "alt+shift+space".into(),
            theme: ThemeMode::Auto,
            spotify_client_id: String::new(),
            hide_in_fullscreen: true,
            compact: CompactMode::Auto,
            whatsapp: true,
            whatsapp_preview: true,
        }
    }
}

impl Config {
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|s| Self::from_json(&s)).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
    }
}

/// Lista de apps criada no primeiro uso. O usuário pode editar `allowlist.json`.
pub fn default_allowlist_json() -> String {
    let mut apps = serde_json::Map::new();
    let mut put = |k: &str, v: &str| {
        apps.insert(k.into(), v.into());
    };
    put("spotify", "spotify:");
    put("bloco de notas", "notepad.exe");
    put("notepad", "notepad.exe");
    put("calculadora", "calc.exe");
    put("explorador", "explorer.exe");
    put("arquivos", "explorer.exe");
    put("configuracoes", "ms-settings:");
    put("terminal", "wt.exe");
    put("paint", "mspaint.exe");
    put("gerenciador de tarefas", "taskmgr.exe");
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let candidates = [
        ("chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string()),
        ("edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".to_string()),
        ("vscode", format!(r"{local}\Programs\Microsoft VS Code\Code.exe")),
        ("vs code", format!(r"{local}\Programs\Microsoft VS Code\Code.exe")),
        ("steam", r"C:\Program Files (x86)\Steam\steam.exe".to_string()),
    ];
    for (name, path) in candidates {
        if Path::new(&path).is_file() {
            put(name, &path);
        }
    }
    serde_json::to_string_pretty(&apps).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing_or_corrupted() {
        let c = Config::from_json("{lixo");
        assert_eq!(c.hotkey, "alt+shift+space");
        assert_eq!(c.theme, ThemeMode::Auto);
        assert_eq!(c.spotify_client_id, "");
        assert!(c.hide_in_fullscreen);
    }
    #[test]
    fn partial_json_keeps_other_defaults() {
        let c = Config::from_json(r#"{"theme":"noite","spotify_client_id":"abc"}"#);
        assert_eq!(c.theme, ThemeMode::Night);
        assert_eq!(c.spotify_client_id, "abc");
        assert_eq!(c.hotkey, "alt+shift+space");
    }
    #[test]
    fn theme_mode_cycles_and_forces() {
        assert_eq!(ThemeMode::Auto.next(), ThemeMode::Day);
        assert_eq!(ThemeMode::Night.next(), ThemeMode::Auto);
        assert_eq!(ThemeMode::Sunset.resolve(23, 0), crate::theme::Period::Sunset);
        assert_eq!(ThemeMode::Auto.resolve(23, 0), crate::theme::Period::Night);
    }
    #[test]
    fn default_allowlist_is_valid_json() {
        let a = crate::launcher::Allowlist::from_json(&default_allowlist_json()).unwrap();
        assert_eq!(a.get("spotify"), Some("spotify:"));
        assert!(a.get("bloco de notas").is_some());
    }
}

#[cfg(test)]
mod compact_tests {
    use super::*;

    #[test]
    fn compact_mode_defaults_cycles_and_decides() {
        assert_eq!(Config::from_json("{}").compact, CompactMode::Auto);
        assert_eq!(Config::from_json(r#"{"compact":"relogio"}"#).compact, CompactMode::Clock);
        assert_eq!(CompactMode::Auto.next(), CompactMode::Music);
        assert_eq!(CompactMode::Clock.next(), CompactMode::Auto);
        assert!(CompactMode::Auto.shows_music(true, true));
        assert!(!CompactMode::Auto.shows_music(true, false), "pausada → relógio");
        assert!(CompactMode::Music.shows_music(true, false));
        assert!(!CompactMode::Music.shows_music(false, false), "sem música → relógio");
        assert!(!CompactMode::Clock.shows_music(true, true));
    }
}

#[cfg(test)]
mod whatsapp_tests {
    use super::*;

    #[test]
    fn whatsapp_options_default_on() {
        let c = Config::from_json("{}");
        assert!(c.whatsapp && c.whatsapp_preview);
        let c = Config::from_json(r#"{"whatsapp_preview":false}"#);
        assert!(c.whatsapp && !c.whatsapp_preview);
    }
}
