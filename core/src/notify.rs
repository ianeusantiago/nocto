//! Avisos do WhatsApp e de e-mail a partir das notificações que os apps mandam para o Windows.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub sender: String,
    pub text: String,
}

pub fn is_whatsapp(app_name: &str) -> bool {
    app_name.to_lowercase().contains("whatsapp")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    WhatsApp,
    /// E-mail; `web` = veio do navegador (Gmail/Outlook na web), senão de um app.
    Email { web: bool },
}

const MAIL_APPS: [&str; 5] = ["outlook", "mail", "thunderbird", "spark", "proton"];
const BROWSERS: [&str; 6] = ["brave", "chrome", "edge", "firefox", "opera", "vivaldi"];
const MAIL_SITES: [&str; 4] = ["mail.google.com", "gmail", "outlook.live.com", "outlook.office"];

/// De onde é a notificação: WhatsApp, e-mail (app de e-mail, ou Gmail/Outlook avisando
/// pelo navegador) ou outra coisa (`None`, ignorada).
pub fn source_of(app_name: &str, texts: &[String]) -> Option<Source> {
    let app = app_name.to_lowercase();
    if app.contains("whatsapp") {
        return Some(Source::WhatsApp);
    }
    if BROWSERS.iter().any(|b| app.contains(b)) {
        let all = texts.join(" ").to_lowercase();
        return MAIL_SITES.iter().any(|s| all.contains(s)).then_some(Source::Email { web: true });
    }
    MAIL_APPS.iter().any(|m| app.contains(m)).then_some(Source::Email { web: false })
}

/// O que abrir ao clicar num aviso de e-mail.
pub fn mail_target(source: Source) -> &'static str {
    match source {
        Source::Email { web: false } => "ms-outlook:",
        _ => "https://mail.google.com",
    }
}

/// Textos da notificação → quem mandou (1º texto) e a mensagem (o resto, juntos).
/// Com `preview` desligado, a mensagem vira "Nova mensagem".
pub fn message_from(texts: &[String], preview: bool) -> Option<Message> {
    let sender = texts.first()?.trim().to_string();
    if sender.is_empty() {
        return None;
    }
    let body: Vec<&str> = texts[1..].iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    let mut text = if preview { body.join(" · ") } else { "Nova mensagem".into() };
    if text.is_empty() {
        text = "Nova mensagem".into();
    }
    if text.chars().count() > 120 {
        text = text.chars().take(120).collect::<String>() + "…";
    }
    Some(Message { sender, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_source() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(source_of("WhatsApp", &t(&["Maria"])), Some(Source::WhatsApp));
        assert_eq!(source_of("Outlook (new)", &t(&["Ana", "Reunião"])), Some(Source::Email { web: false }));
        assert_eq!(source_of("Brave", &t(&["Ana", "Reunião", "mail.google.com"])), Some(Source::Email { web: true }));
        assert_eq!(source_of("Brave", &t(&["YouTube", "vídeo novo"])), None);
        assert_eq!(source_of("Discord", &t(&["oi"])), None);
    }
    #[test]
    fn only_whatsapp_is_accepted() {
        assert!(is_whatsapp("WhatsApp"));
        assert!(is_whatsapp("WhatsApp Beta"));
        assert!(!is_whatsapp("Claude"));
    }
    #[test]
    fn sender_and_message_from_toast_texts() {
        let m = message_from(&["Maria".into(), "Bora jogar hoje?".into()], true).unwrap();
        assert_eq!((m.sender.as_str(), m.text.as_str()), ("Maria", "Bora jogar hoje?"));
        let m = message_from(&["Grupo da Família".into(), "Tio: bom dia".into(), "Mãe: 🙏".into()], true).unwrap();
        assert_eq!(m.text, "Tio: bom dia · Mãe: 🙏");
        assert!(message_from(&[], true).is_none());
    }
    #[test]
    fn hides_text_when_preview_is_off() {
        let m = message_from(&["Maria".into(), "segredo".into()], false).unwrap();
        assert_eq!(m.text, "Nova mensagem");
    }
    #[test]
    fn long_text_is_shortened() {
        let long = "a".repeat(300);
        let m = message_from(&["Maria".into(), long], true).unwrap();
        assert!(m.text.chars().count() <= 121 && m.text.ends_with('…'));
    }
}
