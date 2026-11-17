//! Área de transferência: que tipo de coisa foi copiada e cores de código no estilo do
//! VS Code (tema Dark+). Sem interface, testado.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Link,
    Code,
    Color,
    Image,
    File,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "Texto",
            Kind::Link => "Link",
            Kind::Code => "Código",
            Kind::Color => "Cor",
            Kind::Image => "Imagem",
            Kind::File => "Arquivo",
        }
    }
}

/// "#2bb4ff", "#fff", "rgb(43, 180, 255)" → (r, g, b).
pub fn parse_color(text: &str) -> Option<(u8, u8, u8)> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix('#') {
        let full: String = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect(),
            6 | 8 => hex[..6].to_string(),
            _ => return None,
        };
        let v = u32::from_str_radix(&full, 16).ok()?;
        return Some(((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    let inner = t.strip_prefix("rgb(").or_else(|| t.strip_prefix("rgba("))?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(|p| p.trim().parse::<u8>());
    Some((parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?))
}

fn is_link(t: &str) -> bool {
    let t = t.trim();
    !t.contains(char::is_whitespace)
        && (t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www."))
}

/// Pistas de código: pontuação típica, indentação e palavras de linguagem.
fn looks_like_code(t: &str) -> bool {
    let lines: Vec<&str> = t.lines().collect();
    let mut score = 0;
    let symbols = [";", "{", "}", "=>", "->", "==", "()", "::", "</", "/>", "#include", "&&", "||"];
    score += symbols.iter().filter(|s| t.contains(*s)).count();
    let words = ["fn ", "let ", "const ", "function ", "def ", "class ", "import ", "return ", "public ", "impl ", "var ", "if (", "for (", "#!/"];
    score += 2 * words.iter().filter(|w| t.contains(*w)).count();
    if lines.len() > 1 && lines.iter().filter(|l| l.starts_with("    ") || l.starts_with('\t')).count() >= 1 {
        score += 2;
    }
    // Comando de terminal numa linha só ("cargo build --release", "git push").
    let first = t.trim().split_whitespace().next().unwrap_or("");
    let cli = ["cargo", "git", "npm", "npx", "pnpm", "yarn", "pip", "python", "node", "cd", "ls", "docker", "curl", "winget", "rustup", "gh"];
    if lines.len() == 1 && cli.contains(&first) && t.contains(' ') {
        score += 3;
    }
    score >= 3
}

pub fn classify_text(t: &str) -> Kind {
    if parse_color(t).is_some() {
        Kind::Color
    } else if is_link(t) {
        Kind::Link
    } else if looks_like_code(t) {
        Kind::Code
    } else {
        Kind::Text
    }
}

/// Uma linha do resumo do cartão (sem quebras, até ~90 caracteres).
pub fn summary(t: &str) -> String {
    let one: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 90 { one.chars().take(90).collect::<String>() + "…" } else { one }
}

/// "agora", "2 min", "1 h", "3 d".
pub fn age(secs: u64) -> String {
    match secs {
        0..=59 => "agora".into(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86400),
    }
}

// ───────── Cores de código (VS Code Dark+) ─────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Number,
    Comment,
    Func,
    Type,
    Punct,
}

impl Tok {
    /// Cor RGB do tema Dark+ do VS Code.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Tok::Plain => (0xd4, 0xd4, 0xd4),
            Tok::Keyword => (0x56, 0x9c, 0xd6),
            Tok::Str => (0xce, 0x91, 0x78),
            Tok::Number => (0xb5, 0xce, 0xa8),
            Tok::Comment => (0x6a, 0x99, 0x55),
            Tok::Func => (0xdc, 0xdc, 0xaa),
            Tok::Type => (0x4e, 0xc9, 0xb0),
            Tok::Punct => (0xd4, 0xd4, 0xd4),
        }
    }
}

const KEYWORDS: [&str; 62] = [
    "fn", "let", "mut", "const", "static", "pub", "use", "mod", "impl", "struct", "enum", "trait", "match", "if", "else",
    "for", "while", "loop", "in", "return", "break", "continue", "async", "await", "move", "where", "self", "Self",
    "function", "var", "class", "extends", "new", "import", "from", "export", "default", "def", "and", "or", "not",
    "is", "None", "True", "False", "true", "false", "null", "undefined", "this", "public", "private", "protected",
    "void", "int", "try", "catch", "throw", "lambda", "with", "as", "type",
];

/// Quebra uma linha em pedaços coloridos. `in_block` diz se a linha começa dentro de
/// um comentário /* … */ e é atualizado para a próxima linha.
pub fn highlight_line(line: &str, in_block: &mut bool) -> Vec<(String, Tok)> {
    let c: Vec<char> = line.chars().collect();
    let mut out: Vec<(String, Tok)> = Vec::new();
    let mut push = |s: String, t: Tok| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((prev, pt)) if *pt == t => prev.push_str(&s),
            _ => out.push((s, t)),
        }
    };
    let mut i = 0;
    while i < c.len() {
        if *in_block {
            let start = i;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            if i < c.len() {
                i += 2;
                *in_block = false;
            }
            push(c[start..i.min(c.len())].iter().collect(), Tok::Comment);
            continue;
        }
        let ch = c[i];
        // Comentários de linha: //, # (Python, shell), --
        if (ch == '/' && c.get(i + 1) == Some(&'/')) || (ch == '#' && (i == 0 || c[i - 1].is_whitespace())) {
            push(c[i..].iter().collect(), Tok::Comment);
            break;
        }
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            *in_block = true;
            continue;
        }
        if ch == '"' || ch == '\'' || ch == '`' {
            let start = i;
            i += 1;
            while i < c.len() && c[i] != ch {
                if c[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(c.len());
            push(c[start..i].iter().collect(), Tok::Str);
            continue;
        }
        if ch.is_ascii_digit() {
            let start = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            push(c[start..i].iter().collect(), Tok::Number);
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let next = c[i..].iter().find(|x| !x.is_whitespace());
            let tok = if KEYWORDS.contains(&word.as_str()) {
                Tok::Keyword
            } else if next == Some(&'(') || next == Some(&'!') {
                Tok::Func
            } else if word.chars().next().is_some_and(|f| f.is_uppercase()) {
                Tok::Type
            } else {
                Tok::Plain
            };
            push(word, tok);
            continue;
        }
        let tok = if ch.is_whitespace() { Tok::Plain } else { Tok::Punct };
        push(ch.to_string(), tok);
        i += 1;
    }
    out
}

/// Todas as linhas (até `max`) já coloridas.
pub fn highlight(code: &str, max: usize) -> Vec<Vec<(String, Tok)>> {
    let mut block = false;
    code.lines().take(max).map(|l| highlight_line(&l.replace('\t', "    "), &mut block)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(classify_text("https://github.com/ianeusantiago/nocto"), Kind::Link);
        assert_eq!(classify_text("#2bb4ff"), Kind::Color);
        assert_eq!(classify_text("rgb(43, 180, 255)"), Kind::Color);
        assert_eq!(classify_text("cargo build --release"), Kind::Code);
        assert_eq!(classify_text("fn main() {\n    println!(\"oi\");\n}"), Kind::Code);
        assert_eq!(classify_text("Reunião amanhã às 14h com o time."), Kind::Text);
        assert_eq!(parse_color("#fff"), Some((255, 255, 255)));
    }

    #[test]
    fn highlights_like_vscode() {
        let mut b = false;
        let toks = highlight_line("let x = foo(\"oi\", 42); // fim", &mut b);
        let kinds: Vec<Tok> = toks.iter().map(|t| t.1).collect();
        assert!(toks.iter().any(|(s, t)| s == "let" && *t == Tok::Keyword));
        assert!(toks.iter().any(|(s, t)| s == "foo" && *t == Tok::Func));
        assert!(toks.iter().any(|(s, t)| s == "\"oi\"" && *t == Tok::Str));
        assert!(toks.iter().any(|(s, t)| s == "42" && *t == Tok::Number));
        assert_eq!(kinds.last(), Some(&Tok::Comment));
        let lines = highlight("/* a\nb */ let", 10);
        assert_eq!(lines[0][0].1, Tok::Comment);
        assert_eq!(lines[1].last().unwrap().1, Tok::Keyword);
    }

    #[test]
    fn short_texts() {
        assert_eq!(age(30), "agora");
        assert_eq!(age(150), "2 min");
        assert!(summary(&"a ".repeat(100)).ends_with('…'));
    }
}
