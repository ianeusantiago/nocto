//! Atalho global em texto ("ctrl+alt+space") → modificadores e tecla virtual do Windows.

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}

pub fn parse_hotkey(text: &str) -> Result<Hotkey, String> {
    let mut modifiers = 0;
    let mut vk = None;
    for part in text.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        match part.as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" => modifiers |= MOD_WIN,
            key => {
                if vk.is_some() {
                    return Err(format!("atalho com duas teclas: {text}"));
                }
                vk = Some(key_code(key).ok_or_else(|| format!("tecla desconhecida: {key}"))?);
            }
        }
    }
    let vk = vk.ok_or_else(|| format!("atalho sem tecla: {text}"))?;
    if modifiers == 0 {
        return Err(format!("atalho precisa de ctrl, alt, shift ou win: {text}"));
    }
    Ok(Hotkey { modifiers, vk })
}

fn key_code(key: &str) -> Option<u32> {
    match key {
        "space" | "espaco" => Some(0x20),
        "enter" => Some(0x0D),
        "tab" => Some(0x09),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Some(k.to_ascii_uppercase().as_bytes()[0] as u32)
        }
        k if k.starts_with('f') => match k[1..].parse::<u32>() {
            Ok(n @ 1..=24) => Some(0x6F + n),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        assert_eq!(parse_hotkey("ctrl+alt+space"), Ok(Hotkey { modifiers: MOD_CONTROL | MOD_ALT, vk: 0x20 }));
        assert_eq!(parse_hotkey(" Ctrl + Shift + N "), Ok(Hotkey { modifiers: MOD_CONTROL | MOD_SHIFT, vk: b'N' as u32 }));
        assert_eq!(parse_hotkey("win+f2"), Ok(Hotkey { modifiers: MOD_WIN, vk: 0x71 }));
        assert_eq!(parse_hotkey("alt+7"), Ok(Hotkey { modifiers: MOD_ALT, vk: b'7' as u32 }));
    }
    #[test]
    fn rejects_bad_hotkeys() {
        assert!(parse_hotkey("space").is_err(), "sem modificador");
        assert!(parse_hotkey("ctrl+alt").is_err(), "sem tecla");
        assert!(parse_hotkey("ctrl+banana").is_err());
        assert!(parse_hotkey("").is_err());
    }
}
