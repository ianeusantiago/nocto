//! Avisos do WhatsApp a partir das notificações que o app manda para o Windows.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub sender: String,
    pub text: String,
}

pub fn is_whatsapp(app_name: &str) -> bool {
    app_name.to_lowercase().contains("whatsapp")
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
