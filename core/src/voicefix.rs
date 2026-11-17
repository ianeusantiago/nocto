//! Conserta o que a voz offline entendeu torto ("abril breve" → "abre brave"):
//! aproxima o verbo dos comandos conhecidos e, no "abre", o nome dos apps da lista.

const VERBS: [&str; 10] = ["abre", "toca", "pausa", "próxima", "anterior", "agenda", "volta", "continua", "pausar", "abrir"];

fn plain(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn lev(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + (a[i - 1] != b[j - 1]) as usize);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Parecença de 0 a 1 (sem acento, espaço ou caixa).
fn similar(a: &str, b: &str) -> f32 {
    let (a, b) = (plain(a), plain(b));
    let n = a.chars().count().max(b.chars().count());
    if n == 0 { 0.0 } else { 1.0 - lev(&a, &b) as f32 / n as f32 }
}

fn best<'a>(word: &str, options: impl IntoIterator<Item = &'a str>, min: f32) -> Option<&'a str> {
    options
        .into_iter()
        .map(|o| (o, similar(word, o)))
        .filter(|(_, s)| *s >= min)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(o, _)| o)
}

pub fn fix(text: &str, apps: &[String]) -> String {
    let text = text.trim().to_lowercase();
    let (first, rest) = text.split_once(' ').unwrap_or((&text, ""));
    let verb = best(first, VERBS, 0.5).unwrap_or(first);
    let verb = match verb {
        "abrir" => "abre",
        "pausar" => "pausa",
        v => v,
    };
    let rest = rest.trim();
    if verb == "abre" && !rest.is_empty() {
        if let Some(app) = best(rest, apps.iter().map(|s| s.as_str()), 0.45) {
            return format!("abre {app}");
        }
    }
    if rest.is_empty() { verb.to_string() } else { format!("{verb} {rest}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_garbled_commands() {
        let apps = vec!["brave".to_string(), "spotify".into(), "calculadora".into(), "chrome".into()];
        assert_eq!(fix("abril breve", &apps), "abre brave");
        assert_eq!(fix("abre spot fai", &apps), "abre spotify");
        assert_eq!(fix("pausa", &apps), "pausa");
        assert_eq!(fix("próxima", &apps), "próxima");
        assert_eq!(fix("toca legião urbana", &apps), "toca legião urbana");
        assert_eq!(fix("abre calculadora", &apps), "abre calculadora");
    }
}

/// Como cada app "soa" com palavras que o modelo em português conhece
/// (o modelo não tem palavras em inglês como "spotify").
fn aliases(app: &str) -> Vec<String> {
    let extra: &[&str] = match app {
        "spotify" => &["esporte", "esportes", "esporte fim"],
        "brave" => &["breve", "brava", "bravo"],
        "chrome" => &["cromo"],
        "notepad" => &["note pede", "nota pé", "no pé"],
        "paint" => &["pinte", "pé", "pinta"],
        "steam" => &["time", "estime"],
        "vs code" | "vscode" => &["vê esse code", "vê esse código", "viés code"],
        "configuracoes" => &["configurações", "configuração"],
        _ => &[],
    };
    let mut out = vec![app.to_string()];
    out.extend(extra.iter().map(|s| s.to_string()));
    out
}

const SIMPLE: [&str; 8] = ["pausa", "próxima", "próximo", "anterior", "agenda", "volta", "continua", "toca"];

/// Lista fechada de frases para a voz (gramática do Vosk), em JSON.
/// Com a lista, o reconhecimento só escolhe entre comandos que existem: muito mais certeiro.
pub fn grammar(apps: &[String]) -> String {
    let mut phrases: Vec<String> = SIMPLE.iter().map(|s| s.to_string()).collect();
    phrases.push("toca [unk]".into());
    for app in apps {
        for a in aliases(app) {
            phrases.push(format!("abre {a}"));
            phrases.push(format!("abrir {a}"));
            phrases.push(a);
        }
    }
    phrases.push("[unk]".into());
    serde_json::to_string(&phrases).unwrap_or_default()
}

/// Junta as duas leituras da mesma fala: a da lista fechada (`picked`) e a livre (`free`).
pub fn command(picked: &str, free: &str, apps: &[String]) -> String {
    // Timer é frase livre com números ("bota um timer de três minutos"): usa a leitura livre.
    if ["timer", "temporizador", "cronômetro"].iter().any(|w| free.contains(w)) {
        return free.trim().to_string();
    }
    let picked = picked.replace("[unk]", "").trim().to_string();
    let first = picked.split(' ').next().unwrap_or("");
    if first == "toca" {
        // Nome de música é livre: usa a leitura livre, sem o "toca".
        let free = free.trim();
        let rest = match free.split_once(' ') {
            Some((v, r)) if similar(v, "toca") >= 0.5 => r,
            Some(_) => free,
            None => "",
        };
        return if rest.trim().is_empty() { "toca".into() } else { format!("toca {}", rest.trim()) };
    }
    if SIMPLE.contains(&picked.as_str()) {
        return if picked == "próximo" { "próxima".into() } else { picked };
    }
    let name = picked.strip_prefix("abrir ").or_else(|| picked.strip_prefix("abre ")).unwrap_or(&picked).trim();
    if !name.is_empty() {
        for app in apps {
            if aliases(app).iter().any(|a| a == name) {
                return format!("abre {app}");
            }
        }
    }
    fix(free, apps)
}

#[cfg(test)]
mod grammar_tests {
    use super::*;

    #[test]
    fn picks_command_from_grammar() {
        let apps = vec!["spotify".to_string(), "brave".into(), "calculadora".into()];
        assert!(grammar(&apps).contains("\"abre esporte\""));
        assert_eq!(command("abre esporte", "abre sport", &apps), "abre spotify");
        assert_eq!(command("breve", "", &apps), "abre brave");
        assert_eq!(command("pausa", "pausa", &apps), "pausa");
        assert_eq!(command("toca [unk]", "toca legião urbana", &apps), "toca legião urbana");
        assert_eq!(command("[unk]", "abril breve", &apps), "abre brave");
    }
}
