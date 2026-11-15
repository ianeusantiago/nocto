//! Analisador de comandos fixos da barra (sem IA): regras e palavras-chave.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    OpenApp(String),
    OpenUrl(String),
    OpenFolder(String),
    SpotifyPlay(String),
    SpotifyPlaylist(String),
    SpotifyPause,
    SpotifyNext,
    SpotifyLogin,
    ShowCalendar,
    Quit,
    Ignore,
    Unknown(String),
}

const OPEN: [&str; 5] = ["abre", "abrir", "abra", "open", "abri"];
const PLAY: [&str; 5] = ["toca", "tocar", "toque", "play", "bota"];
const PAUSE: [&str; 5] = ["pausa", "pausar", "pause", "para", "parar"];
const NEXT: [&str; 5] = ["proxima", "next", "pula", "pular", "avanca"];
const ARTICLES: [&str; 6] = ["o", "a", "os", "as", "um", "uma"];

/// Minúsculas e sem acentos, só para comparar palavras-chave.
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

pub fn parse(input: &str) -> Command {
    let input = input.trim();
    if input.is_empty() {
        return Command::Ignore;
    }
    let (first, rest) = match input.split_once(char::is_whitespace) {
        Some((f, r)) => (f, r.trim()),
        None => (input, ""),
    };
    let verb = fold(first);
    let verb = verb.as_str();
    let whole = fold(input);
    let whole = whole.split_whitespace().collect::<Vec<_>>().join(" ");

    match whole.as_str() {
        "agenda" | "calendario" | "abre o calendario" | "abre a agenda" | "abrir agenda" | "abrir calendario" => {
            return Command::ShowCalendar;
        }
        "sair" | "fechar nocto" | "sair do nocto" | "fecha o nocto" => return Command::Quit,
        "login spotify" | "conectar spotify" | "entrar no spotify" => return Command::SpotifyLogin,
        _ => {}
    }

    if rest.is_empty() {
        if PAUSE.contains(&verb) {
            return Command::SpotifyPause;
        }
        if NEXT.contains(&verb) {
            return Command::SpotifyNext;
        }
        return Command::Unknown(input.to_string());
    }
    if PLAY.contains(&verb) {
        let lower = rest.to_lowercase();
        for prefix in ["playlist ", "a playlist ", "minha playlist ", "a minha playlist "] {
            if let Some(name) = lower.strip_prefix(prefix) {
                if !name.trim().is_empty() {
                    return Command::SpotifyPlaylist(name.trim().to_string());
                }
            }
        }
        return Command::SpotifyPlay(lower);
    }
    if OPEN.contains(&verb) {
        return open_target(rest);
    }
    Command::Unknown(input.to_string())
}

fn open_target(rest: &str) -> Command {
    // Tira um artigo inicial ("abre o spotify").
    let target = match rest.split_once(char::is_whitespace) {
        Some((a, r)) if ARTICLES.contains(&fold(a).as_str()) => r.trim(),
        _ => rest,
    };
    let lower = target.to_lowercase();
    let is_path = target.contains(":\\") || target.starts_with("\\\\") || target.starts_with('/');
    if is_path {
        return Command::OpenFolder(target.to_string());
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Command::OpenUrl(target.to_string());
    }
    let looks_like_site = !target.contains(char::is_whitespace)
        && target.contains('.')
        && !lower.ends_with(".exe")
        && !target.starts_with('.')
        && !target.ends_with('.');
    if looks_like_site {
        return Command::OpenUrl(format!("https://{target}"));
    }
    Command::OpenApp(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_app() {
        assert_eq!(parse("abre o spotify"), Command::OpenApp("spotify".into()));
        assert_eq!(parse("Abrir Bloco de Notas"), Command::OpenApp("bloco de notas".into()));
    }
    #[test]
    fn play() {
        assert_eq!(parse("toca bohemian rhapsody"), Command::SpotifyPlay("bohemian rhapsody".into()));
        assert_eq!(parse("toca"), Command::Unknown("toca".into()));
    }
    #[test]
    fn controls() {
        assert_eq!(parse("pausa"), Command::SpotifyPause);
        assert_eq!(parse("próxima"), Command::SpotifyNext);
    }
    #[test]
    fn url() {
        assert_eq!(parse("abre github.com"), Command::OpenUrl("https://github.com".into()));
        assert_eq!(parse("abre https://a.b/c"), Command::OpenUrl("https://a.b/c".into()));
    }
    #[test]
    fn folder() {
        assert_eq!(parse(r"abre C:\Users"), Command::OpenFolder(r"C:\Users".into()));
    }
    #[test]
    fn empty_is_ignored() {
        assert_eq!(parse(""), Command::Ignore);
        assert_eq!(parse("   "), Command::Ignore);
    }
    #[test]
    fn unknown() {
        assert_eq!(parse("faz um bolo"), Command::Unknown("faz um bolo".into()));
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn calendar_and_quit() {
        assert_eq!(parse("agenda"), Command::ShowCalendar);
        assert_eq!(parse("Calendário"), Command::ShowCalendar);
        assert_eq!(parse("abre o calendário"), Command::ShowCalendar);
        assert_eq!(parse("sair"), Command::Quit);
        assert_eq!(parse("fechar nocto"), Command::Quit);
    }
    #[test]
    fn spotify_login() {
        assert_eq!(parse("login spotify"), Command::SpotifyLogin);
        assert_eq!(parse("conectar spotify"), Command::SpotifyLogin);
    }
}

#[cfg(test)]
mod playlist_tests {
    use super::*;

    #[test]
    fn playlist_command() {
        assert_eq!(parse("toca playlist only day"), Command::SpotifyPlaylist("only day".into()));
        assert_eq!(parse("Toca a playlist Only Day"), Command::SpotifyPlaylist("only day".into()));
        assert_eq!(parse("tocar minha playlist only day"), Command::SpotifyPlaylist("only day".into()));
        assert_eq!(parse("toca playlist"), Command::SpotifyPlay("playlist".into()));
        assert_eq!(parse("toca ghost town"), Command::SpotifyPlay("ghost town".into()));
    }
}
