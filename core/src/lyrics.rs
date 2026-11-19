//! Letras para o modo karaokê: busca no LRCLIB (grátis, sem login), lê o formato LRC
//! e diz qual linha está tocando. As letras ficam guardadas em `lyrics/` depois da 1ª vez.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Lyrics {
    /// (início em ms, texto). Em letras sem sincronia, o tempo é 0 em todas.
    pub lines: Vec<(u64, String)>,
    pub synced: bool,
    /// Tempo de cada palavra (início em ms, nº de letras), quando o LRCLIB tem.
    pub words: Vec<Vec<(u64, usize)>>,
    /// Fim de cada linha (ms), quando conhecido.
    pub ends: Vec<Option<u64>>,
}

impl Lyrics {
    fn simple(lines: Vec<(u64, String)>, synced: bool) -> Self {
        let n = lines.len();
        Lyrics { lines, synced, words: vec![Vec::new(); n], ends: vec![None; n] }
    }

    /// Quanto da linha `i` já foi cantado na posição `pos` (0..1).
    pub fn fill(&self, i: usize, pos: u64) -> f32 {
        let Some((start, text)) = self.lines.get(i) else { return 0.0 };
        let next = self.lines.get(i + 1).map(|l| l.0);
        let words = self.words.get(i).map(|w| w.as_slice()).unwrap_or(&[]);
        if words.is_empty() {
            let gap = next.unwrap_or(start + 5000).saturating_sub(*start);
            return line_fill(text, pos.saturating_sub(*start), gap);
        }
        let total: usize = words.iter().map(|w| w.1).sum::<usize>().max(1);
        let end = self.ends.get(i).copied().flatten().or(next).unwrap_or(start + 5000);
        for (k, &(ws, len)) in words.iter().enumerate() {
            if pos < ws {
                break;
            }
            let wend = words.get(k + 1).map(|w| w.0).unwrap_or(end);
            // A palavra enche no tempo dela (sem esticar pelas pausas).
            let dur = wend.saturating_sub(ws).min((len as u64 * 90).max(220)).max(1);
            let frac = ((pos - ws) as f32 / dur as f32).min(1.0);
            let before: usize = words[..k].iter().map(|w| w.1).sum();
            let lit = before as f32 + len as f32 * frac;
            if k + 1 == words.len() || pos < words[k + 1].0 {
                return (lit / total as f32).clamp(0.0, 1.0);
            }
        }
        0.0
    }
}

/// Lê o "lyricsfile" do LRCLIB (YAML simples com o tempo de cada palavra).
pub fn parse_lyricsfile(src: &str) -> Option<Lyrics> {
    let mut lines: Vec<(u64, String)> = Vec::new();
    let mut words: Vec<Vec<(u64, usize)>> = Vec::new();
    let mut ends: Vec<Option<u64>> = Vec::new();
    let mut in_lines = false;
    let mut pending_word: Option<usize> = None;
    for raw in src.lines() {
        if raw == "lines:" {
            in_lines = true;
            continue;
        }
        if !in_lines {
            continue;
        }
        let t = raw.trim_start();
        let indent = raw.len() - t.len();
        if let Some(v) = t.strip_prefix("- text:") {
            let v = yaml_str(v);
            if indent == 0 {
                lines.push((0, v.trim().to_string()));
                words.push(Vec::new());
                ends.push(None);
                pending_word = None;
            } else if let Some(w) = words.last_mut() {
                w.push((0, v.chars().count()));
                pending_word = Some(w.len() - 1);
            }
        } else if let Some(v) = t.strip_prefix("start_ms:") {
            let ms: u64 = v.trim().parse().ok()?;
            match pending_word.take() {
                Some(k) => {
                    if let Some(w) = words.last_mut() {
                        w[k].0 = ms;
                    }
                }
                None => {
                    if let Some(l) = lines.last_mut() {
                        l.0 = ms;
                    }
                }
            }
        } else if let Some(v) = t.strip_prefix("end_ms:") {
            if let (Some(e), Ok(ms)) = (ends.last_mut(), v.trim().parse()) {
                *e = Some(ms);
            }
        } else if indent == 0 {
            in_lines = false;
        }
    }
    if lines.is_empty() || words.iter().all(|w| w.is_empty()) {
        return None;
    }
    Some(Lyrics { lines, synced: true, words, ends })
}

fn yaml_str(v: &str) -> String {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    if let Some(inner) = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return inner.replace("\\\"", "\"");
    }
    v.to_string()
}

/// Lê "[mm:ss.xx] texto" (aceita vários tempos por linha e ignora tags como [ar:]).
pub fn parse_lrc(src: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            if let Some(ms) = parse_time(&stripped[..end]) {
                times.push(ms);
            }
            rest = stripped[end + 1..].trim_start();
        }
        for t in times {
            out.push((t, rest.trim().to_string()));
        }
    }
    out.sort_by_key(|l| l.0);
    out
}

fn parse_time(s: &str) -> Option<u64> {
    let (m, rest) = s.split_once(':')?;
    let m: u64 = m.trim().parse().ok()?;
    let (sec, frac) = rest.split_once('.').unwrap_or((rest, "0"));
    let sec: u64 = sec.trim().parse().ok()?;
    let frac = frac.trim();
    let digits: String = frac.chars().take(3).collect();
    let mut f: u64 = digits.parse().ok()?;
    for _ in digits.len()..3 {
        f *= 10;
    }
    Some(m * 60_000 + sec * 1000 + f)
}

/// Índice da linha atual (a última que já começou), ou None antes da primeira.
pub fn current(lines: &[(u64, String)], pos_ms: u64) -> Option<usize> {
    let n = lines.partition_point(|l| l.0 <= pos_ms);
    n.checked_sub(1)
}

fn from_json(v: &Value) -> Option<Lyrics> {
    if let Some(l) = v["lyricsfile"].as_str().and_then(parse_lyricsfile) {
        return Some(l);
    }
    if let Some(s) = v["syncedLyrics"].as_str() {
        let lines = parse_lrc(s);
        if !lines.is_empty() {
            return Some(Lyrics::simple(lines, true));
        }
    }
    let plain = v["plainLyrics"].as_str()?;
    let lines: Vec<_> = plain.lines().map(|l| (0, l.trim().to_string())).collect();
    (!lines.is_empty()).then_some(Lyrics::simple(lines, false))
}

fn cache_name(artist: &str, title: &str) -> String {
    let key: String = format!("{artist} - {title}")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    format!("{}.json", key.chars().take(120).collect::<String>())
}

/// Busca a letra (primeiro no cache). `None` = sem letra; `Err` = sem conexão.
pub fn fetch(dir: &Path, artist: &str, title: &str, duration_s: u64) -> Result<Option<Lyrics>, String> {
    let cache = dir.join("lyrics2").join(cache_name(artist, title));
    if let Ok(s) = std::fs::read_to_string(&cache) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            return Ok(from_json(&v));
        }
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .user_agent("Nocto (https://github.com/ianeusantiago/nocto)")
        .build()
        .into();
    let get = |url: &str, q: &[(&str, String)]| -> Result<Option<Value>, String> {
        let mut req = agent.get(url);
        for (k, v) in q {
            req = req.query(*k, v);
        }
        let mut resp = req.call().map_err(|e| format!("sem conexão: {e}"))?;
        if resp.status().as_u16() != 200 {
            return Ok(None);
        }
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        Ok(serde_json::from_str(&body).ok())
    };
    let mut q = vec![("artist_name", artist.to_string()), ("track_name", title.to_string())];
    if duration_s > 0 {
        q.push(("duration", duration_s.to_string()));
    }
    let mut found = get("https://lrclib.net/api/get", &q)?;
    if found.as_ref().and_then(from_json).is_none() {
        // Busca solta: pega o primeiro resultado com letra sincronizada (ou qualquer um).
        let list = get("https://lrclib.net/api/search", &[("q", format!("{artist} {title}"))])?;
        let items = list.as_ref().and_then(|v| v.as_array()).cloned().unwrap_or_default();
        found = items
            .iter()
            .find(|v| v["syncedLyrics"].is_string())
            .or_else(|| items.iter().find(|v| v["plainLyrics"].is_string()))
            .cloned();
    }
    let Some(v) = found else { return Ok(None) };
    let slim = serde_json::json!({ "syncedLyrics": v["syncedLyrics"], "plainLyrics": v["plainLyrics"], "lyricsfile": v["lyricsfile"] });
    let _ = std::fs::create_dir_all(dir.join("lyrics2"));
    let _ = std::fs::write(&cache, slim.to_string());
    Ok(from_json(&slim))
}

/// Quanto da linha já foi cantado (0..1), avançando palavra por palavra.
/// O tempo de canto é estimado pelo tamanho da linha (~65 ms por letra), sem passar
/// do intervalo até a próxima linha; assim frases rápidas não ficam para trás.
pub fn line_fill(text: &str, elapsed_ms: u64, gap_ms: u64) -> f32 {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return 0.0;
    }
    let sing = ((n as u64 * 65).clamp(500, 5000)).min(gap_ms.saturating_sub(150).max(300));
    let f = (elapsed_ms as f32 / sing as f32).clamp(0.0, 1.0);
    if f >= 1.0 {
        return 1.0;
    }
    // Acende a palavra inteira assim que ela começa.
    let at = (f * n as f32) as usize;
    let end = (at..n).find(|&i| chars[i] == ' ').unwrap_or(n);
    end as f32 / n as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_timing() {
        let y = "version: '1.0'\nlines:\n- text: I can't blame you, no\n  start_ms: 1000\n  end_ms: 4000\n  words:\n  - text: 'I '\n    start_ms: 1000\n  - text: 'can''t '\n    start_ms: 1200\n  - text: 'no, '\n    start_ms: 1600\n  - text: no\n    start_ms: 3500\n";
        let l = parse_lyricsfile(y).unwrap();
        assert_eq!(l.lines, vec![(1000, "I can't blame you, no".to_string())]);
        assert_eq!(l.words[0], vec![(1000, 2), (1200, 6), (1600, 4), (3500, 2)]);
        // Na pausa entre os dois "no", o segundo ainda não acendeu.
        let f = l.fill(0, 3000);
        assert!((f - 12.0 / 14.0).abs() < 0.01, "{f}");
        assert_eq!(l.fill(0, 3900), 1.0);
    }

    #[test]
    fn fills_by_word() {
        assert_eq!(line_fill("I wanna", 0, 3000), 1.0 / 7.0);
        assert_eq!(line_fill("I wanna", 100, 3000), 1.0 / 7.0);
        assert_eq!(line_fill("I wanna", 200, 3000), 1.0);
        assert_eq!(line_fill("I wanna", 9000, 3000), 1.0);
        // Frase rápida: termina antes da próxima linha.
        assert_eq!(line_fill("one two three four five six", 900, 1000), 1.0);
    }

    #[test]
    fn parses_lrc() {
        let l = parse_lrc("[ar:X]\n[00:12.34] Oi\n[01:02.5][00:01.00]Dois\n[00:20.00]");
        assert_eq!(l, vec![(1000, "Dois".into()), (12340, "Oi".into()), (20000, "".into()), (62500, "Dois".into())]);
    }

    #[test]
    fn finds_current() {
        let l = parse_lrc("[00:01.00]a\n[00:05.00]b");
        assert_eq!(current(&l, 500), None);
        assert_eq!(current(&l, 1000), Some(0));
        assert_eq!(current(&l, 9000), Some(1));
    }
}
