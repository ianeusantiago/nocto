//! Calendário local: tarefas e compromissos num arquivo JSON.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// AAAA-MM-DD
    pub date: String,
    /// HH:MM
    pub time: Option<String>,
    pub title: String,
    pub done: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Calendar {
    entries: Vec<Entry>,
}

impl Calendar {
    /// Nunca falha: arquivo ausente ou corrompido resulta em calendário vazio.
    /// O arquivo corrompido é preservado e copiado para `.json.bak`.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(cal) => cal,
            Err(_) => {
                let _ = std::fs::copy(path, path.with_extension("json.bak"));
                Self::default()
            }
        }
    }

    /// Gravação atômica: escreve num temporário e renomeia.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    pub fn add(&mut self, entry: Entry) {
        self.entries.push(entry);
        self.entries.sort_by(|a, b| (&a.date, &a.time).cmp(&(&b.date, &b.time)));
    }

    /// Itens de um dia, em ordem de horário.
    pub fn on(&self, date: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.date == date).collect()
    }

    /// Posição global do `index`-ésimo item do dia.
    fn position(&self, date: &str, index: usize) -> Option<usize> {
        self.entries.iter().enumerate().filter(|(_, e)| e.date == date).nth(index).map(|(i, _)| i)
    }

    pub fn toggle(&mut self, date: &str, index: usize) {
        if let Some(i) = self.position(date, index) {
            self.entries[i].done = !self.entries[i].done;
        }
    }

    pub fn remove(&mut self, date: &str, index: usize) {
        if let Some(i) = self.position(date, index) {
            self.entries.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nocto-cal-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("calendar.json")
    }
    fn entry() -> Entry {
        Entry { date: "2026-11-20".into(), time: Some("14:00".into()), title: "reunião".into(), done: false }
    }

    #[test]
    fn roundtrip() {
        let p = tmp("rt");
        let mut c = Calendar::load(&p);
        c.add(entry());
        c.save(&p).unwrap();
        let c2 = Calendar::load(&p);
        assert_eq!(c2.on("2026-11-20"), vec![&entry()]);
        assert!(c2.on("2026-11-21").is_empty());
    }
    #[test]
    fn missing_file_gives_empty() {
        assert!(Calendar::load(Path::new("nao_existe.json")).on("2026-11-20").is_empty());
    }
    #[test]
    fn corrupted_file_gives_empty_and_is_not_overwritten() {
        let p = tmp("bad");
        fs::write(&p, "{lixo").unwrap();
        let c = Calendar::load(&p);
        assert!(c.on("2026-11-20").is_empty());
        assert_eq!(fs::read_to_string(&p).unwrap(), "{lixo");
        assert_eq!(fs::read_to_string(p.with_extension("json.bak")).unwrap(), "{lixo");
    }
    #[test]
    fn remove_and_toggle() {
        let mut c = Calendar::default();
        c.add(entry());
        c.toggle("2026-11-20", 0);
        assert!(c.on("2026-11-20")[0].done);
        c.remove("2026-11-20", 0);
        assert!(c.on("2026-11-20").is_empty());
    }
}

/// Um dia da grade do mês.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub in_month: bool,
}

impl Day {
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Dia da semana, 0 = domingo (algoritmo de Sakamoto).
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[m as usize - 1] + d as i32).rem_euclid(7)) as u32
}

/// Grade de 6 semanas (42 dias) começando no domingo.
pub fn month_grid(year: i32, month: u32) -> Vec<Day> {
    let (py, pm) = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let lead = weekday(year, month, 1);
    let prev_len = days_in_month(py, pm);
    let mut out = Vec::with_capacity(42);
    for i in 0..lead {
        out.push(Day { year: py, month: pm, day: prev_len - lead + 1 + i, in_month: false });
    }
    for d in 1..=days_in_month(year, month) {
        out.push(Day { year, month, day: d, in_month: true });
    }
    let mut d = 1;
    while out.len() < 42 {
        out.push(Day { year: ny, month: nm, day: d, in_month: false });
        d += 1;
    }
    out
}

/// "14:00 reunião" → item com horário; "comprar pão" → sem horário.
pub fn parse_entry(date: &str, text: &str) -> Option<Entry> {
    let text = text.trim();
    let (time, title) = match text.split_once(char::is_whitespace) {
        Some((t, rest)) => match parse_time(t) {
            Some(t) => (Some(t), rest.trim()),
            None => (None, text),
        },
        None if parse_time(text).is_some() => return None,
        None => (None, text),
    };
    if title.is_empty() {
        return None;
    }
    Some(Entry { date: date.into(), time, title: title.into(), done: false })
}

fn parse_time(s: &str) -> Option<String> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && s.len() <= 5).then(|| format!("{h:02}:{m:02}"))
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    #[test]
    fn november_2026_starts_on_sunday() {
        let g = month_grid(2026, 11);
        assert_eq!(g.len(), 42);
        assert_eq!(g[0], Day { year: 2026, month: 11, day: 1, in_month: true });
        assert_eq!(g[29], Day { year: 2026, month: 11, day: 30, in_month: true });
        assert_eq!(g[30], Day { year: 2026, month: 12, day: 1, in_month: false });
    }
    #[test]
    fn grid_crosses_year_and_handles_leap() {
        let g = month_grid(2027, 1); // 1 jan 2027 é sexta
        assert_eq!(g[0], Day { year: 2026, month: 12, day: 27, in_month: false });
        assert_eq!(g[5], Day { year: 2027, month: 1, day: 1, in_month: true });
        assert_eq!(month_grid(2028, 2).iter().filter(|d| d.in_month).count(), 29);
        assert_eq!(Day { year: 2026, month: 3, day: 7, in_month: true }.iso(), "2026-03-07");
    }
    #[test]
    fn parse_entry_reads_optional_time() {
        let e = parse_entry("2026-11-20", "14:00 reunião").unwrap();
        assert_eq!((e.time.as_deref(), e.title.as_str()), (Some("14:00"), "reunião"));
        let e = parse_entry("2026-11-20", "9:05 café").unwrap();
        assert_eq!(e.time.as_deref(), Some("09:05"));
        let e = parse_entry("2026-11-20", "comprar pão").unwrap();
        assert_eq!((e.time, e.title.as_str()), (None, "comprar pão"));
        let e = parse_entry("2026-11-20", "25:00 x").unwrap();
        assert_eq!((e.time, e.title.as_str()), (None, "25:00 x"));
        assert!(parse_entry("2026-11-20", "   ").is_none());
        assert!(parse_entry("2026-11-20", "14:00").is_none());
    }
}
