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
