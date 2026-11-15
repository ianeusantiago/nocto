//! Executa comandos de abrir, só pelo que a lista de permissões autoriza.

use crate::commands::Command;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command as Process;

/// Nome do app (minúsculas) → executável ou URI (ex.: "spotify:").
#[derive(Debug, Default, Clone)]
pub struct Allowlist {
    apps: HashMap<String, String>,
}

impl Allowlist {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let apps: HashMap<String, String> =
            serde_json::from_str(json).map_err(|e| format!("allowlist inválida: {e}"))?;
        Ok(Self { apps: apps.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect() })
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| Self::from_json(&s).ok())
            .unwrap_or_default()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.apps.get(&name.to_lowercase()).map(String::as_str)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.apps.keys().map(String::as_str)
    }
}

/// Abre pelo shell do Windows sem passar por `cmd` (sem interpretação de comando).
fn shell_open(target: &str) -> Result<(), String> {
    Process::new("explorer.exe")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir: {e}"))
}

fn safe_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || "\"'<>|^`".contains(c))
}

pub fn run(cmd: &Command, allow: &Allowlist) -> Result<(), String> {
    match cmd {
        Command::OpenApp(name) => {
            let target = allow
                .get(name)
                .ok_or_else(|| format!("\"{name}\" não está na lista de apps permitidos"))?;
            if Path::new(target).is_file() || target.to_lowercase().ends_with(".exe") {
                Process::new(target)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| format!("não consegui abrir {name}: {e}"))
            } else {
                shell_open(target)
            }
        }
        Command::OpenUrl(url) => {
            if !safe_url(url) {
                return Err("endereço inválido".into());
            }
            shell_open(url)
        }
        Command::OpenFolder(path) => {
            if !Path::new(path).is_dir() {
                return Err(format!("pasta não encontrada: {path}"));
            }
            shell_open(path)
        }
        _ => Err("não é um comando de abrir".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;

    #[test]
    fn app_outside_allowlist_is_refused() {
        assert!(run(&Command::OpenApp("virus".into()), &Allowlist::default()).is_err());
    }
    #[test]
    fn url_with_quote_or_space_is_refused() {
        assert!(run(&Command::OpenUrl("https://a.b/\" & calc".into()), &Allowlist::default()).is_err());
        assert!(run(&Command::OpenUrl("file:///c:/x".into()), &Allowlist::default()).is_err());
    }
    #[test]
    fn missing_folder_is_refused() {
        assert!(run(&Command::OpenFolder(r"C:\nao\existe\nocto".into()), &Allowlist::default()).is_err());
    }
    #[test]
    fn allowlist_parses_json() {
        let a = Allowlist::from_json(r#"{"spotify":"spotify:"}"#).unwrap();
        assert_eq!(a.get("spotify"), Some("spotify:"));
        assert!(Allowlist::from_json("{lixo").is_err());
    }
}
