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
const SCOPES: &str = "user-modify-playback-state user-read-playback-state playlist-read-private playlist-read-collaborative";

#[derive(Debug, Clone)]
pub struct Config {
    pub client_id: String,
    pub token_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub uri: String,
    pub label: String,
    pub album: Option<String>,
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
    if status == 403 && body.to_lowercase().contains("scope") {
        return "o Nocto precisa de uma permissão nova do Spotify: digite conectar spotify".into();
    }
    match (status, reason.as_str()) {
        (401, _) => "sessão expirada: faça login no Spotify de novo".into(),
        (_, "PREMIUM_REQUIRED") => "o Spotify exige conta Premium para isso".into(),
        (404, _) | (_, "NO_ACTIVE_DEVICE") => "nenhum player ativo: abra o Spotify e toque algo uma vez".into(),
        // App do Spotify em modo de desenvolvimento: só contas cadastradas pelo dono podem usar.
        (403, _) => "conta não liberada no app do Spotify: o dono do Client ID precisa adicionar seu e-mail em User Management (ou use Premium)".into(),
        _ => format!("o Spotify recusou o pedido ({status})"),
    }
}

pub fn first_track(json: &str) -> Option<Track> {
    let v: Value = serde_json::from_str(json).ok()?;
    let t = v["tracks"]["items"].get(0)?;
    let artists: Vec<&str> = t["artists"].as_array()?.iter().filter_map(|a| a["name"].as_str()).collect();
    Some(Track {
        uri: t["uri"].as_str()?.into(),
        label: format!("{} — {}", t["name"].as_str()?, artists.join(", ")),
        album: t["album"]["uri"].as_str().map(str::to_string),
    })
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

/// Abre o login numa janela pop-up própria (modo app do Edge, presente em todo Windows 10/11);
/// sem Edge, usa o navegador padrão.
fn open_popup(url: &str) -> Result<(), String> {
    let edge = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    if let Some(exe) = edge.iter().find(|p| std::path::Path::new(p).is_file()) {
        let opened = std::process::Command::new(exe)
            .arg(format!("--app={url}"))
            .arg("--window-size=480,760")
            .spawn()
            .is_ok();
        if opened {
            return Ok(());
        }
    }
    crate::launcher::open_url(url)
}

fn result_page(ok: bool) -> String {
    let (title, text) = if ok {
        ("Spotify conectado", "Pode fechar esta janela e voltar ao Nocto.")
    } else {
        ("Login não concluído", "Feche esta janela e tente de novo com “conectar spotify”.")
    };
    format!(
        r#"<!doctype html><html lang="pt-BR"><meta charset="utf-8"><title>Nocto</title>
<body style="margin:0;height:100vh;display:grid;place-items:center;background:#0b2a44;color:#fff;font-family:'Segoe UI Variable','Segoe UI',sans-serif">
<div style="text-align:center;padding:32px 40px;border-radius:24px;background:rgba(255,255,255,.08);border:1px solid rgba(255,255,255,.18)">
<div style="font-size:40px;color:#2bb4ff">{icon}</div><h1 style="font-size:22px;margin:12px 0 6px">{title}</h1>
<p style="margin:0;opacity:.75">{text}</p></div></body></html>"#,
        icon = if ok { "✓" } else { "!" }
    )
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
    open_popup(&url)?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = now() + 180;
    // Navegadores às vezes abrem conexões antecipadas sem pedido; espera a do /callback.
    let code = loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).ok();
                stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                if !line.starts_with("GET /callback") {
                    let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                    continue;
                }
                let code = callback_code(&line, &state);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{}",
                    result_page(code.is_ok())
                );
                break code?;
            }
            Err(_) if now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return Err("tempo de login esgotado".into()),
        }
    };
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
    let (title, artist) = split_query(query);
    let mut track = None;
    if let Some(artist) = &artist {
        let q = format!("track:\"{title}\" artist:\"{artist}\"");
        track = best_track(&search(&auth, &q)?, query);
    }
    if track.is_none() {
        track = best_track(&search(&auth, query)?, query);
    }
    let track = track.ok_or_else(|| format!("não achei \"{query}\" no Spotify"))?;

    // Um único pedido de tocar, já com o aparelho definido: o ativo ou, se o Spotify
    // estiver aberto e parado, o computador da lista de aparelhos.
    let device = match active_device(&get(&auth, "https://api.spotify.com/v1/me/player")?) {
        Some(id) => Some(id),
        None => pick_device(&get(&auth, "https://api.spotify.com/v1/me/player/devices")?),
    };
    let (status, body) = start_playback(&auth, &track, device.as_deref())?;
    if !(200..300).contains(&status) {
        return Err(play_error(status, &body));
    }
    Ok(format!("Tocando: {}", track.label))
}

fn start_playback(auth: &str, track: &Track, device: Option<&str>) -> Result<(u16, String), String> {
    let url = match device {
        Some(id) => format!("https://api.spotify.com/v1/me/player/play?device_id={}", enc(id)),
        None => "https://api.spotify.com/v1/me/player/play".into(),
    };
    let payload = play_payload(track).to_string();
    let mut resp = agent()
        .put(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/json")
        .send(payload)
        .map_err(|e| format!("sem conexão: {e}"))?;
    Ok((resp.status().as_u16(), resp.body_mut().read_to_string().unwrap_or_default()))
}

/// Corpo do pedido de tocar. O app desktop do Spotify ignora (com 204!) um pedido de
/// faixa solta (`uris`) enquanto já toca outra faixa solta; tocar o álbum posicionado
/// na faixa troca sempre (testado na API em 2026-11-15).
pub fn play_payload(track: &Track) -> Value {
    match &track.album {
        Some(album) => serde_json::json!({ "context_uri": album, "offset": { "uri": track.uri }, "position_ms": 0 }),
        None => serde_json::json!({ "uris": [track.uri], "position_ms": 0 }),
    }
}

/// Música e posição atuais no Spotify (para o karaokê). `None` se nada toca ou sem login.
pub fn now_playing(cfg: &Config) -> Option<(String, u64, bool)> {
    if cfg.client_id.is_empty() || !cfg.token_path.exists() {
        return None;
    }
    let token = access_token(cfg).ok()?;
    let body = get(&format!("Bearer {token}"), "https://api.spotify.com/v1/me/player/currently-playing").ok()?;
    let v: Value = serde_json::from_str(&body).ok()?;
    Some((v["item"]["name"].as_str()?.to_string(), v["progress_ms"].as_u64()?, v["is_playing"].as_bool().unwrap_or(false)))
}

fn get(auth: &str, url: &str) -> Result<String, String> {
    let mut resp = agent().get(url).header("Authorization", auth).call().map_err(|e| format!("sem conexão: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    match status {
        200..=299 => Ok(body),
        _ => Err(play_error(status, &body)),
    }
}

fn search(auth: &str, q: &str) -> Result<String, String> {
    get(auth, &format!("https://api.spotify.com/v1/search?type=track&limit=10&q={}", enc(q)))
}

/// Minúsculas, sem acento, só letras/números separados por um espaço.
fn norm(s: &str) -> String {
    let folded: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'ê' | 'ë' => 'e',
            'í' | 'ï' => 'i',
            'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "ghost town do kanye west" → ("ghost town", Some("kanye west")). Só separa por
/// " do ", " da ", " by " e " - " (" de " é comum demais em títulos).
pub fn split_query(query: &str) -> (String, Option<String>) {
    let q = norm(query);
    for sep in [" do ", " da ", " by ", " - "] {
        if let Some((t, a)) = q.split_once(sep) {
            if !t.is_empty() && !a.is_empty() {
                return (t.to_string(), Some(a.to_string()));
            }
        }
    }
    (q, None)
}

/// Escolhe, entre os resultados da busca, o que mais se parece com o pedido.
pub fn best_track(json: &str, query: &str) -> Option<Track> {
    let v: Value = serde_json::from_str(json).ok()?;
    let (title, artist) = split_query(query);
    let compact = |s: &str| s.replace(' ', "");
    let q_words: Vec<String> = norm(query).split(' ').map(str::to_string).collect();
    let q_compact = compact(&norm(query));
    let mut best: Option<(i32, Track)> = None;
    for (rank, t) in v["tracks"]["items"].as_array()?.iter().enumerate() {
        let (Some(name), Some(uri)) = (t["name"].as_str(), t["uri"].as_str()) else { continue };
        let artists: Vec<&str> = t["artists"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect()).unwrap_or_default();
        let n = norm(name);
        let a = norm(&artists.join(" "));
        let full = format!("{n} {a}");
        let mut score = 0;
        if compact(&n) == compact(&title) {
            score += 100; // título exato, mesmo com palavras grudadas ("ghosttown")
        } else if compact(&full) == q_compact || q_compact.starts_with(&compact(&n)) && !n.is_empty() {
            score += 60;
        }
        let hits = q_words.iter().filter(|w| full.split(' ').any(|f| f == w.as_str())).count() as i32;
        score += 30 * hits / q_words.len().max(1) as i32;
        if let Some(artist) = &artist {
            if a.contains(artist.as_str()) {
                score += 40;
            }
        }
        score -= rank as i32; // empate: mantém a ordem do Spotify (popularidade)
        let label = format!("{name} — {}", artists.join(", "));
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            let album = t["album"]["uri"].as_str().map(str::to_string);
            best = Some((score, Track { uri: uri.into(), label, album }));
        }
    }
    best.map(|(_, t)| t)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub name: String,
    pub uri: String,
}

/// Escolhe a playlist pelo nome: igual (sem diferenciar maiúsculas, acentos e espaços)
/// ganha de "contém", que ganha de "tem todas as palavras".
pub fn pick_playlist(json: &str, query: &str) -> Option<Playlist> {
    let v: Value = serde_json::from_str(json).ok()?;
    let q = norm(query);
    let qc = q.replace(' ', "");
    if qc.is_empty() {
        return None;
    }
    let mut best: Option<(i32, Playlist)> = None;
    for item in v["items"].as_array()? {
        let (Some(name), Some(uri)) = (item["name"].as_str(), item["uri"].as_str()) else { continue };
        let n = norm(name);
        let nc = n.replace(' ', "");
        let score = if nc == qc {
            300
        } else if nc.contains(&qc) {
            200 - (nc.len() as i32 - qc.len() as i32).min(100)
        } else if q.split(' ').all(|w| n.split(' ').any(|x| x == w)) {
            100
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, Playlist { name: name.into(), uri: uri.into() }));
        }
    }
    best.map(|(_, p)| p)
}

/// Toca uma playlist sua pelo nome. Retorna "Tocando a playlist …".
pub fn play_playlist(cfg: &Config, name: &str) -> Result<String, String> {
    if cfg.client_id.is_empty() {
        return Err(missing_id());
    }
    let auth = format!("Bearer {}", access_token(cfg)?);
    // Junta até 200 playlists (4 páginas de 50).
    let mut all = Vec::new();
    let mut url = Some("https://api.spotify.com/v1/me/playlists?limit=50".to_string());
    for _ in 0..4 {
        let Some(u) = url.take() else { break };
        let v: Value = serde_json::from_str(&get(&auth, &u)?).unwrap_or_default();
        if let Some(items) = v["items"].as_array() {
            all.extend(items.iter().cloned());
        }
        url = v["next"].as_str().map(str::to_string);
    }
    let json = serde_json::json!({ "items": all }).to_string();
    let playlist = pick_playlist(&json, name).ok_or_else(|| format!("não achei a playlist \"{name}\" nas suas playlists"))?;
    let device = match active_device(&get(&auth, "https://api.spotify.com/v1/me/player")?) {
        Some(id) => Some(id),
        None => pick_device(&get(&auth, "https://api.spotify.com/v1/me/player/devices")?),
    };
    let url = match &device {
        Some(id) => format!("https://api.spotify.com/v1/me/player/play?device_id={}", enc(id)),
        None => "https://api.spotify.com/v1/me/player/play".into(),
    };
    let mut resp = agent()
        .put(&url)
        .header("Authorization", &auth)
        .header("Content-Type", "application/json")
        .send(serde_json::json!({ "context_uri": playlist.uri }).to_string())
        .map_err(|e| format!("sem conexão: {e}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(play_error(status, &resp.body_mut().read_to_string().unwrap_or_default()));
    }
    Ok(format!("Tocando a playlist {}", playlist.name))
}

/// Escolhe o álbum: nome igual ganha de "contém"; o artista pedido conta muito.
pub fn pick_album(json: &str, query: &str) -> Option<Playlist> {
    let v: Value = serde_json::from_str(json).ok()?;
    let (title, artist) = split_query(query);
    let tc = title.replace(' ', "");
    let mut best: Option<(i32, Playlist)> = None;
    for item in v["albums"]["items"].as_array()? {
        let (Some(name), Some(uri)) = (item["name"].as_str(), item["uri"].as_str()) else { continue };
        let n = norm(name).replace(' ', "");
        let mut score = if n == tc {
            300
        } else if n.starts_with(&tc) {
            // "Late Registration (Deluxe)"
            250
        } else if n.contains(&tc) {
            150
        } else {
            continue;
        };
        let artists: Vec<String> = item["artists"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).map(norm).collect()).unwrap_or_default();
        if let Some(a) = &artist {
            if artists.iter().any(|x| x == a || x.contains(a.as_str())) {
                score += 200;
            } else {
                score -= 100;
            }
        }
        if item["album_type"].as_str() == Some("album") {
            score += 10;
        }
        if score > 0 && best.as_ref().is_none_or(|(s, _)| score > *s) {
            let by = item["artists"][0]["name"].as_str().unwrap_or("");
            let label = if by.is_empty() { name.to_string() } else { format!("{name} — {by}") };
            best = Some((score, Playlist { name: label, uri: uri.into() }));
        }
    }
    best.map(|(_, p)| p)
}

/// Toca um álbum inteiro pelo nome (e artista, se vier). Retorna "Tocando o álbum …".
pub fn play_album(cfg: &Config, query: &str) -> Result<String, String> {
    if cfg.client_id.is_empty() {
        return Err(missing_id());
    }
    let auth = format!("Bearer {}", access_token(cfg)?);
    let (title, artist) = split_query(query);
    let find = |q: &str| -> Result<Option<Playlist>, String> {
        let json = get(&auth, &format!("https://api.spotify.com/v1/search?type=album&limit=10&q={}", enc(q)))?;
        Ok(pick_album(&json, query))
    };
    let mut album = None;
    if let Some(a) = &artist {
        album = find(&format!("album:\"{title}\" artist:\"{a}\""))?;
    }
    if album.is_none() {
        album = find(query)?;
    }
    let album = album.ok_or_else(|| format!("não achei o álbum \"{query}\" no Spotify"))?;
    play_context(&auth, &album.uri)?;
    Ok(format!("Tocando o álbum {}", album.name))
}

/// Manda o Spotify tocar um álbum/playlist (no aparelho ativo ou no computador).
fn play_context(auth: &str, uri: &str) -> Result<(), String> {
    let device = match active_device(&get(auth, "https://api.spotify.com/v1/me/player")?) {
        Some(id) => Some(id),
        None => pick_device(&get(auth, "https://api.spotify.com/v1/me/player/devices")?),
    };
    let url = match &device {
        Some(id) => format!("https://api.spotify.com/v1/me/player/play?device_id={}", enc(id)),
        None => "https://api.spotify.com/v1/me/player/play".into(),
    };
    let mut resp = agent()
        .put(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/json")
        .send(serde_json::json!({ "context_uri": uri }).to_string())
        .map_err(|e| format!("sem conexão: {e}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(play_error(status, &resp.body_mut().read_to_string().unwrap_or_default()));
    }
    Ok(())
}

/// Aparelho do player atual, se houver um tocando ou pausado.
pub fn active_device(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    v["device"]["id"].as_str().map(str::to_string)
}

/// Aparelho para tocar: de preferência um computador; ignora os restritos.
pub fn pick_device(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    let devices = v["devices"].as_array()?;
    let usable = |d: &&Value| !d["is_restricted"].as_bool().unwrap_or(false) && d["id"].is_string();
    devices
        .iter()
        .filter(usable)
        .find(|d| d["type"] == "Computer")
        .or_else(|| devices.iter().find(usable))
        .and_then(|d| d["id"].as_str().map(str::to_string))
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
            Some(Track { uri: "spotify:track:abc".into(), label: "Bohemian Rhapsody — Queen".into(), album: None })
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

#[cfg(test)]
mod device_tests {
    use super::*;

    #[test]
    fn prefers_this_computer_and_skips_restricted() {
        let json = r#"{"devices":[
            {"id":"tv","type":"TV","is_restricted":false,"is_active":false},
            {"id":"lock","type":"Computer","is_restricted":true,"is_active":false},
            {"id":"pc","type":"Computer","is_restricted":false,"is_active":false}]}"#;
        assert_eq!(pick_device(json), Some("pc".into()));
    }
    #[test]
    fn falls_back_to_any_usable_device() {
        assert_eq!(pick_device(r#"{"devices":[{"id":"cel","type":"Smartphone","is_restricted":false}]}"#), Some("cel".into()));
        assert_eq!(pick_device(r#"{"devices":[]}"#), None);
        assert_eq!(pick_device("lixo"), None);
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    fn results(items: &[(&str, &str)]) -> String {
        let items: Vec<_> = items
            .iter()
            .enumerate()
            .map(|(i, (n, a))| serde_json::json!({"name": n, "uri": format!("spotify:track:{i}"), "artists": [{"name": a}]}))
            .collect();
        serde_json::json!({"tracks": {"items": items}}).to_string()
    }

    #[test]
    fn glued_words_match_the_real_title() {
        let json = results(&[("ghostin", "Ariana Grande"), ("Ghost Town", "Kanye West")]);
        assert_eq!(best_track(&json, "ghosttown").unwrap().label, "Ghost Town — Kanye West");
    }
    #[test]
    fn title_beats_artist_only_match() {
        let json = results(&[("Monster", "Kanye West"), ("Ghost Town", "Kanye West")]);
        assert_eq!(best_track(&json, "ghost town kanye west").unwrap().label, "Ghost Town — Kanye West");
    }
    #[test]
    fn artist_filter_breaks_ties() {
        let json = results(&[("Ghost Town", "Madonna"), ("Ghost Town", "Kanye West")]);
        assert_eq!(best_track(&json, "ghost town do kanye west").unwrap().label, "Ghost Town — Kanye West");
        assert_eq!(best_track(&results(&[]), "x"), None);
    }
    #[test]
    fn splits_title_and_artist() {
        assert_eq!(split_query("ghost town do kanye west"), ("ghost town".into(), Some("kanye west".into())));
        assert_eq!(split_query("Hello by Adele"), ("hello".into(), Some("adele".into())));
        assert_eq!(split_query("festa de boa"), ("festa de boa".into(), None));
    }
    #[test]
    fn active_device_comes_from_player_state() {
        assert_eq!(active_device(r#"{"is_playing":true,"device":{"id":"pc1","is_active":true}}"#), Some("pc1".into()));
        assert_eq!(active_device(""), None);
    }
}

#[cfg(test)]
mod payload_tests {
    use super::*;

    #[test]
    fn plays_through_album_context_when_known() {
        let t = Track { uri: "spotify:track:1".into(), label: "x".into(), album: Some("spotify:album:9".into()) };
        assert_eq!(
            play_payload(&t),
            serde_json::json!({"context_uri": "spotify:album:9", "offset": {"uri": "spotify:track:1"}, "position_ms": 0})
        );
        let solo = Track { album: None, ..t };
        assert_eq!(play_payload(&solo), serde_json::json!({"uris": ["spotify:track:1"], "position_ms": 0}));
    }
    #[test]
    fn search_result_carries_album() {
        let json = r#"{"tracks":{"items":[{"name":"Ghost Town","uri":"spotify:track:1","album":{"uri":"spotify:album:9"},"artists":[{"name":"Kanye West"}]}]}}"#;
        assert_eq!(best_track(json, "ghost town").unwrap().album.as_deref(), Some("spotify:album:9"));
    }
}

#[cfg(test)]
mod playlist_tests {
    use super::*;

    fn lists(names: &[&str]) -> String {
        let items: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, n)| serde_json::json!({"name": n, "uri": format!("spotify:playlist:{i}")}))
            .collect();
        serde_json::json!({ "items": items, "next": null }).to_string()
    }

    #[test]
    fn exact_name_wins_ignoring_case_and_spaces() {
        let json = lists(&["Daily Mix 1", "Only Day", "Only Day Remix"]);
        assert_eq!(pick_playlist(&json, "only day").unwrap().name, "Only Day");
        assert_eq!(pick_playlist(&json, "onlyday").unwrap().uri, "spotify:playlist:1");
    }
    #[test]
    fn partial_match_and_missing() {
        let json = lists(&["Músicas pra treinar", "Lo-fi"]);
        assert_eq!(pick_playlist(&json, "treinar").unwrap().name, "Músicas pra treinar");
        assert!(pick_playlist(&json, "rock pesado").is_none());
        assert!(pick_playlist("lixo", "x").is_none());
    }
    #[test]
    fn asks_for_playlist_permission() {
        assert!(auth_url("cid", "c", "s").contains("playlist-read-private"));
        assert!(play_error(403, r#"{"error":{"message":"Insufficient client scope"}}"#).contains("conectar spotify"));
    }

    #[test]
    fn picks_album() {
        let json = r#"{"albums":{"items":[
            {"name":"Late Orchestration","uri":"u1","album_type":"album","artists":[{"name":"Kanye West"}]},
            {"name":"Late Registration (Deluxe)","uri":"u2","album_type":"album","artists":[{"name":"Kanye West"}]},
            {"name":"Late Registration","uri":"u3","album_type":"album","artists":[{"name":"Tribute Band"}]}
        ]}}"#;
        let a = pick_album(json, "late registration by kanye west").unwrap();
        assert_eq!(a.uri, "u2");
        assert_eq!(a.name, "Late Registration (Deluxe) — Kanye West");
        assert!(pick_album(json, "graduation by kanye west").is_none());
    }
}
