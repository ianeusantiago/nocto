//! Timer da pílula: entende "bota um timer de 3 minutos", "timer 1h30", "cancela o timer".

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerCmd {
    /// Começar um timer com tantos segundos.
    Start(u32),
    Cancel,
}

const WORDS: [&str; 3] = ["timer", "temporizador", "cronometro"];
const CANCEL: [&str; 8] = ["cancela", "cancelar", "para", "parar", "tira", "tirar", "desliga", "remove"];

/// Números por extenso que a voz devolve ("três minutos").
fn word_number(w: &str) -> Option<u32> {
    Some(match w {
        "um" | "uma" => 1,
        "dois" | "duas" => 2,
        "tres" => 3,
        "quatro" => 4,
        "cinco" => 5,
        "seis" => 6,
        "sete" => 7,
        "oito" => 8,
        "nove" => 9,
        "dez" => 10,
        "onze" => 11,
        "doze" => 12,
        "treze" => 13,
        "quatorze" | "catorze" => 14,
        "quinze" => 15,
        "dezesseis" => 16,
        "dezessete" => 17,
        "dezoito" => 18,
        "dezenove" => 19,
        "vinte" => 20,
        "trinta" => 30,
        "quarenta" => 40,
        "cinquenta" => 50,
        "sessenta" => 60,
        "noventa" => 90,
        _ => return None,
    })
}

fn unit_secs(w: &str) -> Option<u32> {
    match w {
        "s" | "seg" | "segs" | "segundo" | "segundos" | "sec" => Some(1),
        "m" | "min" | "mins" | "minuto" | "minutos" => Some(60),
        "h" | "hr" | "hrs" | "hora" | "horas" => Some(3600),
        _ => None,
    }
}

/// Separa "1h30", "3min", "90s" em número e unidade.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == ',') {
        let mut cur = String::new();
        let mut digit = None;
        for c in word.chars() {
            let d = c.is_ascii_digit();
            if digit.is_some_and(|prev| prev != d) && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(c);
            digit = Some(d);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// `text` já em minúsculas e sem acentos.
pub fn parse(text: &str) -> Option<TimerCmd> {
    let words = tokens(text);
    if !words.iter().any(|w| WORDS.contains(&w.as_str())) {
        return None;
    }
    if words.iter().any(|w| CANCEL.contains(&w.as_str())) {
        return Some(TimerCmd::Cancel);
    }
    let mut total = 0u32;
    let mut pending: Option<u32> = None; // número esperando a unidade
    let mut last_unit = 60u32;
    let mut prev_unit = 60u32; // a última unidade dita (para "e meia")
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        // "um/uma" só vale como número antes de uma unidade ("uma hora"); senão é artigo.
        let article = matches!(w, "um" | "uma") && !words.get(i + 1).is_some_and(|x| unit_secs(x).is_some());
        let n = if article { None } else { w.parse::<u32>().ok().or_else(|| word_number(w)) };
        if let Some(n) = n {
            // "vinte e cinco": soma as dezenas e as unidades por extenso.
            let n = if words.get(i + 1).map(|s| s.as_str()) == Some("e") {
                match words.get(i + 2).and_then(|x| word_number(x)) {
                    Some(m) if n >= 20 && m < 10 => {
                        i += 2;
                        n + m
                    }
                    _ => n,
                }
            } else {
                n
            };
            if let Some(p) = pending.take() {
                total += p * last_unit;
            }
            pending = Some(n);
        } else if w == "meia" || w == "meio" {
            // "meia hora", "uma hora e meia", "um minuto e meio"
            if pending.is_none() && words.get(i + 1).map(|s| s.as_str()).and_then(unit_secs) == Some(3600) {
                total += 1800;
                i += 1;
            } else if pending.is_none() {
                total += prev_unit / 2;
            }
        } else if let Some(u) = unit_secs(w) {
            if let Some(p) = pending.take() {
                total += p * u;
            }
            prev_unit = u;
            // Depois de "1h", um número solto ("1h30") vale minutos; depois de min, segundos.
            last_unit = match u {
                3600 => 60,
                60 => 1,
                _ => 1,
            };
        }
        i += 1;
    }
    if let Some(p) = pending {
        // Número sem unidade nenhuma ("timer 5"): minutos.
        total += p * if total == 0 { 60 } else { last_unit };
    }
    (total > 0 && total <= 24 * 3600).then_some(TimerCmd::Start(total))
}

/// "2:59", "59:00", "1:02:03".
pub fn clock(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// "3 minutos", "1 h 30 min", "45 segundos".
pub fn describe(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    let plural = |n: u32, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match (h, m, s) {
        (0, 0, s) => plural(s, "segundo", "segundos"),
        (0, m, 0) => plural(m, "minuto", "minutos"),
        (0, m, s) => format!("{m} min {s} s"),
        (h, 0, 0) => plural(h, "hora", "horas"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn understands_timer_phrases() {
        assert_eq!(parse("bota um timer de 3 minutos"), Some(TimerCmd::Start(180)));
        assert_eq!(parse("timer 3 min"), Some(TimerCmd::Start(180)));
        assert_eq!(parse("timer 3min"), Some(TimerCmd::Start(180)));
        assert_eq!(parse("timer 5"), Some(TimerCmd::Start(300)));
        assert_eq!(parse("timer 90s"), Some(TimerCmd::Start(90)));
        assert_eq!(parse("timer de 1h30"), Some(TimerCmd::Start(5400)));
        assert_eq!(parse("timer de 2 minutos e 30 segundos"), Some(TimerCmd::Start(150)));
        assert_eq!(parse("coloca um timer de tres minutos"), Some(TimerCmd::Start(180)));
        assert_eq!(parse("timer de vinte e cinco minutos"), Some(TimerCmd::Start(1500)));
        assert_eq!(parse("timer de meia hora"), Some(TimerCmd::Start(1800)));
        assert_eq!(parse("timer de uma hora e meia"), Some(TimerCmd::Start(5400)));
        assert_eq!(parse("cancela o timer"), Some(TimerCmd::Cancel));
        assert_eq!(parse("para o timer"), Some(TimerCmd::Cancel));
        assert_eq!(parse("toca ghost town"), None);
        assert_eq!(parse("timer"), None);
    }

    #[test]
    fn formats_time() {
        assert_eq!(clock(179), "2:59");
        assert_eq!(clock(3723), "1:02:03");
        assert_eq!(describe(180), "3 minutos");
        assert_eq!(describe(5400), "1 h 30 min");
        assert_eq!(describe(45), "45 segundos");
    }
}
