//! "Toca X" pela Web API do Spotify (OAuth PKCE, sem segredo no binário).
//! Só roda quando o usuário pede; nada fica rodando em segundo plano.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const REDIRECT: &str = "http://127.0.0.1:8898/callback";
const SCOPES: &str = "user-modify-playback-state user-read-playback-state";

#[derive(Debug, Clone)]
pub struct Config {
    pub client_id: String,
    pub token_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub uri: String,
    pub label: String,
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn random_token() -> String {
    let mut b = [0u8; 48];
    getrandom::fill(&mut b).expect("gerador aleatório do sistema");
    URL_SAFE_NO_PAD.encode(b)
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn auth_url(client_id: &str, challenge: &str, state: &str) -> String {
    format!(
        "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={}",
        enc(client_id), enc(SCOPES), enc(REDIRECT), enc(challenge), enc(state)
    )
}

/// Lê `code` da primeira linha da requisição de retorno e confere o `state`.
pub fn callback_code(request_line: &str, state: &str) -> Result<String, String> {
    let query = request_line.split_whitespace().nth(1).and_then(|p| p.split_once('?')).map(|(_, q)| q).unwrap_or("");
    let get = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}=")).map(str::to_string));
    if get("state").as_deref() != Some(state) {
        return Err("resposta de login inválida".into());
    }
    if let Some(e) = get("error") {
        return Err(format!("login recusado: {e}"));
    }
    get("code").ok_or_else(|| "login sem código".into())
}

/// Mensagem clara para erros do comando de tocar.
pub fn play_error(status: u16, body: &str) -> String {
    let reason = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["reason"].as_str().map(str::to_string))
        .unwrap_or_default();
    match (status, reason.as_str()) {
        (401, _) => "sessão expirada: faça login no Spotify de novo".into(),
        (_, "PREMIUM_REQUIRED") => "o Spotify exige conta Premium para isso".into(),
        (404, _) | (_, "NO_ACTIVE_DEVICE") => "nenhum player ativo: abra o Spotify e toque algo uma vez".into(),
        _ => format!("o Spotify recusou o pedido ({status})"),
    }
}

pub fn first_track(json: &str) -> Option<Track> {
    let v: Value = serde_json::from_str(json).ok()?;
    let t = v["tracks"]["items"].get(0)?;
    let artists: Vec<&str> = t["artists"].as_array()?.iter().filter_map(|a| a["name"].as_str()).collect();
    Some(Track { uri: t["uri"].as_str()?.into(), label: format!("{} — {}", t["name"].as_str()?, artists.join(", ")) })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
    expires_at: u64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn token_request(cfg: &Config, form: &[(&str, &str)], old_refresh: Option<&str>) -> Result<Tokens, String> {
    let mut resp = agent()
        .post("https://accounts.spotify.com/api/token")
        .send_form(form.iter().copied())
        .map_err(|e| format!("sem conexão com o Spotify: {e}"))?;
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&body).map_err(|_| "resposta inválida do Spotify".to_string())?;
    let access = v["access_token"].as_str().ok_or("login no Spotify falhou")?.to_string();
    let refresh = v["refresh_token"].as_str().or(old_refresh).unwrap_or_default().to_string();
    let t = Tokens { access, refresh, expires_at: now() + v["expires_in"].as_u64().unwrap_or(3600) - 60 };
    if let Ok(json) = serde_json::to_string(&t) {
        let _ = std::fs::write(&cfg.token_path, json);
    }
    Ok(t)
}

/// Abre o navegador para o login e espera o retorno em 127.0.0.1:8898 (até 2 min).
pub fn login(cfg: &Config) -> Result<(), String> {
    if cfg.client_id.is_empty() {
        return Err(missing_id());
    }
    let verifier = random_token();
    let state = random_token();
    let listener = TcpListener::bind("127.0.0.1:8898").map_err(|e| format!("porta 8898 ocupada: {e}"))?;
    let url = auth_url(&cfg.client_id, &pkce_challenge(&verifier), &state);
    std::process::Command::new("explorer.exe").arg(&url).spawn().map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = now() + 120;
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(s) => break s,
            Err(_) if now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return Err("tempo de login esgotado".into()),
        }
    };
    stream.set_nonblocking(false).ok();
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).map_err(|e| e.to_string())?;
    let code = callback_code(&line, &state);
    let page = if code.is_ok() { "Pronto! Pode fechar esta aba e voltar ao Nocto." } else { "Login nao concluido." };
    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n{page}");
    let code = code?;
    token_request(
        cfg,
        &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", REDIRECT), ("client_id", &cfg.client_id), ("code_verifier", &verifier)],
        None,
    )
    .map(|_| ())
}

fn access_token(cfg: &Config) -> Result<String, String> {
    let saved: Option<Tokens> = std::fs::read_to_string(&cfg.token_path).ok().and_then(|s| serde_json::from_str(&s).ok());
    let t = match saved {
        Some(t) if t.expires_at > now() => t,
        Some(t) if !t.refresh.is_empty() => token_request(
            cfg,
            &[("grant_type", "refresh_token"), ("refresh_token", &t.refresh), ("client_id", &cfg.client_id)],
            Some(&t.refresh),
        )?,
        _ => {
            login(cfg)?;
            return access_token(cfg);
        }
    };
    Ok(t.access)
}

fn missing_id() -> String {
    "configure o Client ID do Spotify em config.json (veja o README)".into()
}

/// Busca a música e toca no player ativo. Retorna "Tocando: …".
pub fn play_query(cfg: &Config, query: &str) -> Result<String, String> {
    if cfg.client_id.is_empty() {
        return Err(missing_id());
    }
    let token = access_token(cfg)?;
    let auth = format!("Bearer {token}");
    let url = format!("https://api.spotify.com/v1/search?type=track&limit=1&q={}", enc(query));
    let mut resp = agent().get(&url).header("Authorization", &auth).call().map_err(|e| format!("sem conexão: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    if status != 200 {
        return Err(play_error(status, &body));
    }
    let track = first_track(&body).ok_or_else(|| format!("não achei \"{query}\" no Spotify"))?;
    let payload = serde_json::json!({ "uris": [track.uri] }).to_string();
    let mut resp = agent()
        .put("https://api.spotify.com/v1/me/player/play")
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .send(payload)
        .map_err(|e| format!("sem conexão: {e}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(play_error(status, &resp.body_mut().read_to_string().unwrap_or_default()));
    }
    Ok(format!("Tocando: {}", track.label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[test]
    fn no_active_device_gives_clear_message() {
        let body = r#"{"error":{"status":404,"message":"Player command failed: No active device found","reason":"NO_ACTIVE_DEVICE"}}"#;
        assert!(play_error(404, body).contains("abra o Spotify"));
        assert!(play_error(403, r#"{"error":{"reason":"PREMIUM_REQUIRED"}}"#).contains("Premium"));
        assert!(play_error(401, "").contains("login"));
    }
    #[test]
    fn search_result_is_parsed() {
        let json = r#"{"tracks":{"items":[{"name":"Bohemian Rhapsody","uri":"spotify:track:abc","artists":[{"name":"Queen"}]}]}}"#;
        assert_eq!(
            first_track(json),
            Some(Track { uri: "spotify:track:abc".into(), label: "Bohemian Rhapsody — Queen".into() })
        );
        assert_eq!(first_track(r#"{"tracks":{"items":[]}}"#), None);
        assert_eq!(first_track("lixo"), None);
    }
    #[test]
    fn auth_url_has_pkce_and_loopback_redirect() {
        let u = auth_url("cid", "chal", "st");
        assert!(u.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(u.contains("client_id=cid") && u.contains("code_challenge=chal") && u.contains("state=st"));
        assert!(u.contains("code_challenge_method=S256"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8898%2Fcallback"));
    }
    #[test]
    fn callback_request_is_parsed() {
        assert_eq!(callback_code("GET /callback?code=abc&state=st HTTP/1.1", "st"), Ok("abc".into()));
        assert!(callback_code("GET /callback?code=abc&state=outro HTTP/1.1", "st").is_err());
        assert!(callback_code("GET /callback?error=access_denied&state=st HTTP/1.1", "st").is_err());
    }
    #[test]
    fn missing_client_id_is_clear() {
        let cfg = Config { client_id: String::new(), token_path: "nao_existe.json".into() };
        assert!(play_query(&cfg, "x").unwrap_err().contains("Client ID"));
    }
}
