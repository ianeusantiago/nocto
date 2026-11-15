//! Tema por horário: Dia 05:00–15:59, Pôr do sol 16:00–17:59, Noite 18:00–04:59.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Day,
    Sunset,
    Night,
}

/// Cores RGBA de um período.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub glass: [u8; 4],
    pub accent: [u8; 4],
    pub text: [u8; 4],
}

/// Horas (em segundos desde a meia-noite) em que o tema vira.
const SWITCHES: [u32; 3] = [5 * 3600, 16 * 3600, 18 * 3600];

pub fn period_for(hour: u32, _minute: u32) -> Period {
    match hour {
        5..=15 => Period::Day,
        16..=17 => Period::Sunset,
        _ => Period::Night,
    }
}

/// Tempo até a próxima virada de tema.
pub fn next_switch_in(hour: u32, minute: u32, second: u32) -> Duration {
    let now = hour * 3600 + minute * 60 + second;
    let next = SWITCHES
        .iter()
        .copied()
        .find(|&s| s > now)
        .unwrap_or(SWITCHES[0] + 24 * 3600);
    Duration::from_secs((next - now) as u64)
}

pub fn palette(p: Period) -> Palette {
    match p {
        Period::Day => Palette {
            glass: [0x6c, 0xbd, 0xf5, 0x99],
            accent: [0x00, 0x71, 0xe3, 0xff],
            text: [0xff, 0xff, 0xff, 0xff],
        },
        Period::Sunset => Palette {
            glass: [0xf0, 0x8a, 0x3c, 0x99],
            accent: [0xff, 0xd2, 0x8a, 0xff],
            text: [0xff, 0xff, 0xff, 0xff],
        },
        Period::Night => Palette {
            glass: [0x0b, 0x2a, 0x44, 0xb3],
            accent: [0x2b, 0xb4, 0xff, 0xff],
            text: [0xff, 0xff, 0xff, 0xff],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries() {
        assert_eq!(period_for(5, 0), Period::Day);
        assert_eq!(period_for(15, 59), Period::Day);
        assert_eq!(period_for(16, 0), Period::Sunset);
        assert_eq!(period_for(17, 59), Period::Sunset);
        assert_eq!(period_for(18, 0), Period::Night);
        assert_eq!(period_for(4, 59), Period::Night);
        assert_eq!(period_for(0, 0), Period::Night);
    }

    #[test]
    fn next_switch() {
        assert_eq!(next_switch_in(15, 59, 0).as_secs(), 60);
        assert_eq!(next_switch_in(17, 59, 30).as_secs(), 30);
        assert_eq!(next_switch_in(23, 0, 0).as_secs(), 6 * 3600);
        assert_eq!(next_switch_in(5, 0, 0).as_secs(), 11 * 3600);
    }

    #[test]
    fn night_matches_ina_family() {
        assert_eq!(palette(Period::Night).accent[..3], [0x2b, 0xb4, 0xff]);
        assert_eq!(palette(Period::Night).glass[..3], [0x0b, 0x2a, 0x44]);
    }
}
