//! Textos curtos da ilha: tempo da música, progresso e relógio em português.

use crate::calendar::weekday;

/// 83_400 ms → "1:23".
pub fn mmss(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Fração tocada, entre 0 e 1.
pub fn progress(position_ms: u64, duration_ms: u64) -> f32 {
    if duration_ms == 0 {
        return 0.0;
    }
    (position_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
}

/// Posição agora: o Windows informa a posição de vez em quando; entre uma informação
/// e outra o Nocto conta o tempo sozinho (só enquanto toca).
pub fn position_now(reported_ms: u64, elapsed_ms: u64, playing: bool, duration_ms: u64) -> u64 {
    let pos = if playing { reported_ms + elapsed_ms } else { reported_ms };
    if duration_ms > 0 { pos.min(duration_ms) } else { pos }
}

pub fn clock(hour: u32, minute: u32) -> String {
    format!("{hour:02}:{minute:02}")
}

/// "dom, 15 nov".
pub fn date_label(year: i32, month: u32, day: u32) -> String {
    const WD: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
    const MO: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
    format!("{}, {} {}", WD[weekday(year, month, day) as usize], day, MO[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_time() {
        assert_eq!(mmss(0), "0:00");
        assert_eq!(mmss(83_400), "1:23");
        assert_eq!(mmss(225_999), "3:45");
        assert_eq!(mmss(3_725_000), "62:05");
    }
    #[test]
    fn progress_is_clamped() {
        assert_eq!(progress(30_000, 120_000), 0.25);
        assert_eq!(progress(500_000, 120_000), 1.0);
        assert_eq!(progress(10, 0), 0.0);
    }
    #[test]
    fn clock_label_in_portuguese() {
        assert_eq!(clock(9, 5), "09:05");
        assert_eq!(date_label(2026, 11, 15), "dom, 15 nov");
        assert_eq!(date_label(2027, 1, 1), "sex, 1 jan");
    }
    #[test]
    fn position_advances_only_while_playing() {
        assert_eq!(position_now(10_000, 2_500, true, 200_000), 12_500);
        assert_eq!(position_now(10_000, 2_500, false, 200_000), 10_000);
        assert_eq!(position_now(199_000, 5_000, true, 200_000), 200_000);
    }
}
