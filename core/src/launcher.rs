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

    pub fn sorted_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.apps.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn add(&mut self, name: &str, target: &str) {
        self.apps.insert(name.to_lowercase(), target.to_string());
    }

    pub fn remove(&mut self, name: &str) {
        self.apps.remove(&name.to_lowercase());
    }

    pub fn to_json(&self) -> String {
        let sorted: std::collections::BTreeMap<_, _> = self.apps.iter().collect();
        serde_json::to_string_pretty(&sorted).unwrap_or_else(|_| "{}".into())
    }
}

/// Nome do app a partir de um executável ou atalho arrastado ("Brave.lnk" → "brave").
/// `None` se o arquivo não for um app.
pub fn app_name(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    if ext != "exe" && ext != "lnk" {
        return None;
    }
    Some(path.file_stem()?.to_str()?.trim().to_lowercase())
}

/// Abre pelo shell do Windows sem passar por `cmd` (sem interpretação de comando).
fn shell_open(target: &str) -> Result<(), String> {
    Process::new("explorer.exe")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir: {e}"))
}

/// Abre um endereço no navegador padrão. O `explorer.exe` não entende URLs longas com `&`
/// (abria a pasta Documentos); o manipulador de protocolo do Windows entende.
pub fn open_url(url: &str) -> Result<(), String> {
    Process::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir o navegador: {e}"))
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
            let lower = target.to_lowercase();
            if lower.ends_with(".lnk") {
                // Atalho: quem sabe abrir é o shell do Windows.
                shell_open(target)
            } else if Path::new(target).is_file() || lower.ends_with(".exe") {
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
            open_url(url)
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

#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn add_remove_and_save() {
        let mut a = Allowlist::from_json(r#"{"spotify":"spotify:"}"#).unwrap();
        a.add("Brave", r"C:\Users\x\Desktop\Brave.lnk");
        assert_eq!(a.get("brave"), Some(r"C:\Users\x\Desktop\Brave.lnk"));
        assert_eq!(a.sorted_names(), vec!["brave".to_string(), "spotify".to_string()]);
        let again = Allowlist::from_json(&a.to_json()).unwrap();
        assert_eq!(again.get("brave"), a.get("brave"));
        a.remove("BRAVE");
        assert_eq!(a.get("brave"), None);
    }
    #[test]
    fn app_name_comes_from_the_file() {
        assert_eq!(app_name(Path::new(r"C:\Users\x\Desktop\Brave.lnk")), Some("brave".into()));
        assert_eq!(app_name(Path::new(r"C:\Program Files\Obs\obs64.exe")), Some("obs64".into()));
        assert_eq!(app_name(Path::new(r"C:\x\Visual Studio Code.lnk")), Some("visual studio code".into()));
        assert_eq!(app_name(Path::new(r"C:\x\foto.png")), None);
    }
}
