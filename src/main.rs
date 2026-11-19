#![windows_subsystem = "windows"]

mod clipboard;
mod devices;
mod glass;
mod media;
mod notify;
mod voice;
mod wake;
mod volume;
mod win;

use nocto_core::calendar::{self, Calendar};
use nocto_core::commands::{self, Command};
use nocto_core::config::{self, CompactMode, Config};
use nocto_core::imgconv::{self, Format};
use nocto_core::launcher::{self, Allowlist};
use nocto_core::{clip, format, keys, spotify, theme};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

slint::include_modules!();
use nocto_core::notify::Source;

const PILL: i32 = 0;
const BAR: i32 = 1;
const CALENDAR: i32 = 2;
const DROP: i32 = 3;
const NOTIFY: i32 = 4;
/// Client ID do app do Nocto no Spotify, usado se o config.json não tiver um.
const DEFAULT_SPOTIFY_CLIENT_ID: &str = "29d3c1690e7c4e2791e0d5f4c0e9ab9a";

const MONTHS: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho",
    "julho", "agosto", "setembro", "outubro", "novembro", "dezembro",
];
const WEEKDAYS: [&str; 7] = ["Domingo", "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado"];

struct App {
    ui: Island,
    hwnd: isize,
    dir: PathBuf,
    config: Config,
    allow: Allowlist,
    cal: Calendar,
    view: (i32, u32),
    selected: String,
    prev_foreground: isize,
    dropped: Option<PathBuf>,
    /// Um app em tela cheia (ex.: jogo) está na frente: a pílula some.
    fullscreen_app: bool,
    /// Uma janela cobre o topo da tela, e o mouse está ou não espiando a pílula.
    covered: bool,
    peek: bool,
    glass: Option<glass::Glass>,
    last_shape: std::cell::Cell<(f32, f32)>,
    region_spec: std::cell::Cell<(f32, f32, f32, f32)>,
    hidden: std::cell::Cell<bool>,
    /// Última faixa informada pelo Windows (sem a capa, que já foi para a tela).
    track: Option<media::Track>,
    /// Página do painel: 0 música, 1 calendário.
    page: i32,
    progress_timer: slint::Timer,
    karaoke: bool,
    karaoke_timer: slint::Timer,
    lyrics: Option<nocto_core::lyrics::Lyrics>,
    lyrics_key: String,
    sync_tick: u32,
    /// Relógio suave do karaokê (ms) e quando foi avançado pela última vez.
    k_pos: f64,
    /// Linha atual do karaokê (para animar a troca).
    k_idx: Option<usize>,
    k_clock: Option<std::time::Instant>,
    /// Última vez que a posição veio da API do Spotify.
    synced_at: Option<std::time::Instant>,
    /// Aviso que substitui a data na pílula (ex.: atalho ocupado).
    pill_note: String,
    volume: volume::Volume,
    /// Conta os avisos para cada um fechar só a si mesmo.
    notify_serial: u64,
    /// Histórico da área de transferência (mais novo primeiro) e o item aberto no preview.
    clips: Vec<Clip>,
    clip_open: Option<usize>,
    clip_serial: u64,
    /// Timer da pílula: quando acaba, quanto durava e o tique de 1 s.
    timer_end: Option<std::time::Instant>,
    timer_total: u32,
    timer_tick: slint::Timer,
    /// Escuta do "Nocto" (None = desligada).
    wake: Option<wake::Wake>,
    wake_paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Últimas mensagens do WhatsApp, para a página delas.
    wa_msgs: Vec<WaMsg>,
    /// Última mensagem (origem com hora, quem mandou, texto) para o Início.
    last_msg: Option<(String, String, String)>,
    /// Bateria, câmera/microfone e fones (lidos em segundo plano).
    dev: DevState,
    flash_serial: u64,
    /// Arquivo cuja miniatura está na tela de soltar (clique abre no Explorador).
    preview_path: std::cell::RefCell<Option<PathBuf>>,
    mail_msgs: Vec<WaMsg>,
    /// De onde veio o último e-mail e o último aviso (para saber o que abrir no clique).
    mail_source: Source,
    notify_source: Source,
    /// Ícones dos apps já lidos (destino → ícone), para não reler a cada abertura.
    icons: RefCell<std::collections::HashMap<String, Option<slint::Image>>>,
}

/// Janela fixa (lógica) onde a ilha se transforma; cabe o maior modo (calendário).
const WINDOW_W: f32 = 720.0;
const WINDOW_H: f32 = 720.0;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Acessa o estado na thread da interface. Se ele já estiver em uso (evento disparado
/// no meio de outra ação), a ação é adiada para o próximo giro do laço de eventos.
fn with_app(f: impl FnOnce(&mut App) + 'static) {
    APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            if let Some(app) = slot.as_mut() {
                f(app);
            }
        }
        Err(_) => slint::Timer::single_shot(Duration::ZERO, move || with_app(f)),
    });
}

/// Agenda `f` na thread da interface a partir de qualquer thread.
fn on_ui(f: impl FnOnce(&mut App) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || with_app(f));
}

/// Cores da letra do karaokê ("auto" muda com o período do dia).
const KARAOKE_COLORS: [(&str, &str, [u8; 4]); 6] = [
    ("amarelo", "Amarelo", [0xff, 0xd1, 0x66, 0xff]),
    ("rosa", "Rosa", [0xff, 0x6f, 0xae, 0xff]),
    ("azul", "Azul", [0x4c, 0xc9, 0xff, 0xff]),
    ("verde", "Verde", [0x7e, 0xe0, 0x81, 0xff]),
    ("lilas", "Lilás", [0xb4, 0x9b, 0xff, 0xff]),
    ("branco", "Branco", [0xff, 0xff, 0xff, 0xff]),
];

fn karaoke_rgb(name: &str, period: theme::Period) -> [u8; 4] {
    if let Some(c) = KARAOKE_COLORS.iter().find(|c| c.0 == name) {
        return c.2;
    }
    match period {
        theme::Period::Day => [0xff, 0xd1, 0x66, 0xff],
        theme::Period::Sunset => [0xff, 0x6f, 0xae, 0xff],
        theme::Period::Night => [0xb4, 0x9b, 0xff, 0xff],
    }
}

fn karaoke_label(name: &str) -> &'static str {
    KARAOKE_COLORS.iter().find(|c| c.0 == name).map(|c| c.1).unwrap_or("Automática")
}

fn next_karaoke_color(name: &str) -> &'static str {
    match KARAOKE_COLORS.iter().position(|c| c.0 == name) {
        None => KARAOKE_COLORS[0].0,
        Some(i) if i + 1 < KARAOKE_COLORS.len() => KARAOKE_COLORS[i + 1].0,
        Some(_) => "auto",
    }
}

fn color(c: [u8; 4]) -> Color {
    Color::from_argb_u8(c[3], c[0], c[1], c[2])
}

/// O que o leitor de aparelhos encontrou na última volta.
#[derive(Default, Clone)]
struct DevState {
    read: bool,
    bat: Option<devices::Battery>,
    cam: Vec<String>,
    mic: Vec<String>,
    headsets: Vec<devices::Headset>,
}

/// Lê bateria e câmera/microfone a cada 2 s e os fones a cada 10 s (leitura barata).
fn watch_devices() {
    std::thread::spawn(|| {
        let mut headsets = Vec::new();
        for round in 0u64.. {
            if round % 5 == 0 {
                headsets = devices::bluetooth_batteries();
            }
            let state = DevState {
                read: true,
                bat: devices::battery(),
                cam: devices::in_use("webcam"),
                mic: devices::in_use("microphone"),
                headsets: headsets.clone(),
            };
            on_ui(move |app| app.apply_devices(state));
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn iso_today() -> String {
    let (y, m, d, ..) = win::local_time();
    format!("{y:04}-{m:02}-{d:02}")
}

impl App {
    /// A janela da interface e, atrás dela, a do vidro.
    fn hwnds(&self) -> Vec<isize> {
        std::iter::once(self.hwnd).chain(self.glass.as_ref().map(|g| g.hwnd)).collect()
    }

    /// Área visível e clicável das duas janelas (pixels físicos).
    fn region(&self, x: f32, w: f32, h: f32, r: f32) {
        self.region_spec.set((x, w, h, r));
        self.apply_region();
    }

    /// Some/aparece sem esconder as janelas: esconder (ShowWindow) fazia o Windows
    /// descartar o desenho da interface, que voltava vazia (só o vidro) ou com manchas.
    /// Aqui a área visível vira vazia e o desenho continua intacto.
    /// A pílula some em jogo de tela cheia e quando uma janela cobre o topo
    /// (navegador maximizado). Encostar o mouse no topo mostra de novo.
    fn pill_hidden(&self) -> bool {
        self.fullscreen_app || (self.covered && !self.peek && self.ui.get_pill_flash().is_empty())
    }

    /// Confere a janela da frente e o mouse (a cada 0,5 s; custa quase nada).
    fn check_cover(&mut self) {
        // Desenvolvimento: NOCTO_NOCOVER ignora janelas cobrindo o topo (para prints).
        let covered = std::env::var_os("NOCTO_NOCOVER").is_none() && win::covers_top(win::foreground());
        // Já espiando: continua enquanto o mouse estiver sobre a pílula.
        let peek = covered && win::cursor_at_top(if self.peek { 60 } else { 2 });
        if (covered, peek) != (self.covered, self.peek) {
            self.covered = covered;
            self.peek = peek;
            if self.ui.get_mode() == PILL {
                self.layout();
            }
        }
    }

    fn visible(&self, visible: bool) {
        self.ui.set_pill_visible(visible);
        if self.hidden.replace(!visible) != !visible {
            self.apply_region();
        }
    }

    /// Faz o Slint redesenhar e copiar a janela inteira para a tela (ver `repaint` na UI).
    fn force_repaint(&self) {
        self.ui.set_repaint(self.ui.get_repaint().wrapping_add(1));
    }

    fn apply_region(&self) {
        self.force_repaint();
        let (x, w, h, r) = self.region_spec.get();
        for hwnd in self.hwnds() {
            if self.hidden.get() {
                win::clear_region(hwnd);
            } else {
                win::set_region(hwnd, x, w, h, r);
            }
        }
    }

    /// Largura, altura e raio (lógicos) da ilha em cada modo, como na Dynamic Island:
    /// compacta quando parada, expandindo para barra, calendário ou soltar arquivo.
    fn island_shape(&self) -> (f32, f32, f32) {
        let ui = &self.ui;
        match ui.get_mode() {
            BAR if ui.get_status().is_empty() => (600.0, 58.0, 29.0),
            BAR => (600.0, 86.0, 32.0),
            CALENDAR if self.page == 0 => (680.0, if self.karaoke { 376.0 } else { 214.0 }, 40.0),
            CALENDAR if self.page == 5 => (680.0, 262.0, 40.0),
            CALENDAR if self.page == 4 => (680.0, 698.0, 40.0),
            CALENDAR if self.page == 3 => (680.0, self.clip_height(), 40.0),
            NOTIFY => (440.0, 64.0, 32.0),
            CALENDAR => (680.0, 340.0, 40.0),
            DROP if self.ui.get_has_drop_preview() => (580.0, 150.0, 40.0),
            DROP => (440.0, 128.0, 36.0),
            // Escondida: encolhe até um risquinho no topo antes de sumir (e cresce dele ao voltar).
            PILL if self.pill_hidden() => (90.0, 0.0, 0.0),
            _ if !ui.get_pill_flash().is_empty() => (320.0, 36.0, 18.0),
            _ if ui.get_timer_on() => (210.0, 36.0, 18.0),
            _ if ui.get_show_music() => (360.0 + self.privacy_dots(), 38.0, 19.0),
            _ => (230.0 + self.privacy_dots(), 34.0, 17.0),
        }
    }

    fn layout(&self) {
        let (w, h, r) = self.island_shape();
        let scale = self.ui.window().scale_factor();
        let x = (WINDOW_W - w) / 2.0;
        self.ui.set_island_w(w);
        self.ui.set_island_h(h);
        if let Some(glass) = &self.glass {
            let _ = glass.set_shape(x * scale, w * scale, h * scale, r * scale, true);
        }
        // Durante o morph a área clicável cobre o maior dos dois formatos; no fim, o exato.
        let (pw, ph) = self.last_shape.replace((w, h));
        let (uw, uh) = (w.max(pw), h.max(ph));
        self.region((WINDOW_W - uw) / 2.0 * scale, uw * scale, (uh + 8.0) * scale, r * scale);
        let shape = (w, h, r);
        slint::Timer::single_shot(Duration::from_millis(glass::MORPH_MS as u64 + 80), move || {
            with_app(move |app| {
                if app.island_shape() == shape {
                    let x = (WINDOW_W - w) / 2.0 * scale;
                    app.region(x, w * scale, h * scale, r * scale);
                }
            })
        });
        if self.ui.get_mode() == PILL && self.pill_hidden() {
            // Deixa a animação de encolher terminar antes de esconder.
            if !self.hidden.get() {
                slint::Timer::single_shot(Duration::from_millis(glass::MORPH_MS as u64), || {
                    with_app(|app| {
                        if app.ui.get_mode() == PILL && app.pill_hidden() {
                            app.visible(false);
                        }
                    })
                });
            }
        } else {
            self.visible(true);
        }
    }

    fn set_mode(&mut self, mode: i32) {
        self.ui.set_mode(mode);
        if mode == CALENDAR {
            self.refresh_calendar();
            self.refresh_volume();
        }
        self.layout();
        self.update_progress_timer();
    }

    fn go_page(&mut self, page: i32) {
        if page == self.page {
            return;
        }
        self.page = page;
        self.ui.set_slide(true);
        self.ui.set_page(page);
        if page == 1 {
            self.refresh_calendar();
        }
        if page == 2 {
            self.mark_read();
        }
        if page == 3 {
            self.refresh_clips();
        }
        if page == 4 {
            self.refresh_settings();
        }
        if page == 5 {
            self.refresh_home();
        }
        self.layout();
        self.update_progress_timer();
    }

    /// Mensagem nova do WhatsApp: a ilha se expande por alguns segundos, como no iPhone.
    /// Não interrompe a barra, o painel nem um jogo em tela cheia.
    fn show_message(&mut self, source: Source, texts: Vec<String>) {
        if !self.config.whatsapp {
            return;
        }
        let Some(msg) = nocto_core::notify::message_from(&texts, self.config.whatsapp_preview) else { return };
        let mail = source != Source::WhatsApp;
        // Guarda na página de mensagens (as 5 mais novas no topo de cada aba).
        let now = win::local_time();
        let time = format!("{:02}:{:02}", now.3, now.4);
        let item = WaMsg { sender: msg.sender.clone().into(), text: msg.text.clone().into(), time: time.clone().into() };
        self.last_msg = Some((format!("{} · {time}", if mail { "E-mail" } else { "WhatsApp" }), msg.sender.clone(), msg.text.clone()));
        let list = if mail { &mut self.mail_msgs } else { &mut self.wa_msgs };
        list.insert(0, item);
        list.truncate(5);
        let model = ModelRc::new(VecModel::from(list.clone()));
        if mail {
            self.mail_source = source;
            self.ui.set_mail_msgs(model);
        } else {
            self.ui.set_wa_msgs(model);
        }
        let mode = self.ui.get_mode();
        if mode == CALENDAR && self.page == 2 && self.ui.get_msg_tab() == mail as i32 {
            return;
        }
        if mail {
            self.ui.set_mail_unread(self.ui.get_mail_unread() + 1);
        } else {
            self.ui.set_wa_unread(self.ui.get_wa_unread() + 1);
        }
        self.notify_source = source;
        self.ui.set_notify_timer(false);
        self.ui.set_notify_mail(mail);
        if self.fullscreen_app || (mode != PILL && mode != NOTIFY) {
            return;
        }
        self.ui.set_notify_sender(msg.sender.into());
        self.ui.set_notify_text(msg.text.into());
        self.set_mode(NOTIFY);
        self.notify_serial += 1;
        let serial = self.notify_serial;
        slint::Timer::single_shot(Duration::from_secs(5), move || {
            with_app(move |app| {
                if app.ui.get_mode() == NOTIFY && app.notify_serial == serial {
                    app.set_mode(PILL);
                }
            })
        });
    }

    /// Começa (ou troca) o timer da pílula.
    fn start_timer(&mut self, secs: u32) {
        self.timer_total = secs;
        self.timer_end = Some(std::time::Instant::now() + Duration::from_secs(secs as u64));
        self.ui.set_timer_on(true);
        self.tick_timer();
        // Um tique por segundo, só enquanto há timer.
        self.timer_tick.start(slint::TimerMode::Repeated, Duration::from_secs(1), || with_app(|app| app.tick_timer()));
        if self.ui.get_mode() == PILL {
            self.layout();
        }
    }

    fn stop_timer(&mut self) {
        self.timer_end = None;
        self.timer_tick.stop();
        self.ui.set_timer_on(false);
        if self.ui.get_mode() == PILL {
            self.layout();
        }
    }

    fn tick_timer(&mut self) {
        let Some(end) = self.timer_end else { return };
        let left = end.saturating_duration_since(std::time::Instant::now());
        let secs = (left.as_millis() as u32).div_ceil(1000);
        self.ui.set_timer_text(nocto_core::timer::clock(secs).into());
        self.ui.set_timer_left(secs as f32 / self.timer_total.max(1) as f32);
        if secs == 0 {
            let total = self.timer_total;
            self.stop_timer();
            self.timer_done(total);
        }
    }

    /// Fim do timer: som do Windows e o banner na ilha (mesmo com a pílula escondida
    /// atrás do navegador; em jogo de tela cheia, só o som).
    fn timer_done(&mut self, total: u32) {
        win::alert_sound();
        let mode = self.ui.get_mode();
        if self.fullscreen_app || (mode != PILL && mode != NOTIFY) {
            return;
        }
        self.ui.set_notify_timer(true);
        self.ui.set_notify_sender("Timer acabou".into());
        self.ui.set_notify_text(format!("{} se passaram", nocto_core::timer::describe(total)).into());
        self.set_mode(NOTIFY);
        self.notify_serial += 1;
        let serial = self.notify_serial;
        slint::Timer::single_shot(Duration::from_secs(8), move || {
            with_app(move |app| {
                if app.ui.get_mode() == NOTIFY && app.notify_serial == serial {
                    app.set_mode(PILL);
                }
            })
        });
    }

    // ───────── Área de transferência ─────────

    /// Altura da ilha na página de cópias: cresce com o conteúdo, como a Dynamic Island.
    fn clip_height(&self) -> f32 {
        let Some(i) = self.clip_open else {
            let rows = self.clips.len().div_ceil(3).clamp(1, 3) as f32;
            return if self.clips.is_empty() { 200.0 } else { 120.0 + rows * 96.0 };
        };
        let Some(c) = self.clips.get(i) else { return 220.0 };
        match &c.data {
            clipboard::Data::Image(w, h, _) => {
                let shown = (640.0 * *h as f32 / (*w).max(1) as f32).clamp(160.0, 420.0);
                shown + 130.0
            }
            clipboard::Data::Files(f) => (150.0 + f.len().min(8) as f32 * 20.0).min(360.0),
            clipboard::Data::Text(t) => match c.kind {
                clip::Kind::Code => (132.0 + t.lines().count().min(20) as f32 * 18.0).max(240.0),
                clip::Kind::Color => 250.0,
                clip::Kind::Link => 200.0,
                _ if t.len() > 280 || t.lines().count() > 5 => 460.0,
                _ => (170.0 + (t.len() as f32 / 70.0).ceil() * 22.0).min(300.0),
            },
        }
    }

    /// Algo foi copiado: entra no topo da lista (sem repetir) e a pílula avisa.
    fn add_clip(&mut self, data: clipboard::Data) {
        let kind = match &data {
            clipboard::Data::Text(t) => clip::classify_text(t),
            clipboard::Data::Files(_) => clip::Kind::File,
            clipboard::Data::Image(..) => clip::Kind::Image,
        };
        let same = |c: &Clip| match (&c.data, &data) {
            (clipboard::Data::Text(a), clipboard::Data::Text(b)) => a == b,
            (clipboard::Data::Files(a), clipboard::Data::Files(b)) => a == b,
            (clipboard::Data::Image(aw, ah, a), clipboard::Data::Image(bw, bh, b)) => aw == bw && ah == bh && a == b,
            _ => false,
        };
        let mut pinned = false;
        if let Some(pos) = self.clips.iter().position(same) {
            pinned = self.clips.remove(pos).pinned;
        }
        let thumb = match &data {
            clipboard::Data::Image(w, h, px) => {
                let (tw, th, t) = imgconv::downscale_rgba(*w, *h, px, 900);
                Some(slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&t, tw, th)))
            }
            _ => None,
        };
        let flash = match &data {
            clipboard::Data::Text(t) => clip::summary(t),
            clipboard::Data::Files(f) => f.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            clipboard::Data::Image(w, h, _) => format!("imagem {w}×{h}"),
        };
        self.clips.insert(0, Clip { data, kind, at: std::time::SystemTime::now(), pinned, thumb });
        // Guarda os 20 mais novos; os fixados não saem.
        let mut kept = 0;
        self.clips.retain(|c| {
            kept += 1;
            c.pinned || kept <= 20
        });
        self.clip_open = None;
        if self.ui.get_mode() == CALENDAR && self.page == 3 {
            self.refresh_clips();
            self.layout();
        }
        if self.config.clip_flash {
            self.flash_pill("\u{E73E}", "Copiado", None, &flash, 1400);
        }
    }

    /// Aviso rápido na pílula (copiado, carregando, fone conectado, bateria baixa).
    fn flash_pill(&mut self, icon: &str, title: &str, tint: Option<[u8; 4]>, text: &str, ms: u64) {
        if self.ui.get_mode() != PILL || self.fullscreen_app {
            return;
        }
        self.ui.set_flash_icon(icon.into());
        self.ui.set_flash_title(title.into());
        self.ui.set_flash_color(tint.map(color).unwrap_or(Color::from_argb_u8(0, 0, 0, 0)));
        self.ui.set_pill_flash(if text.is_empty() { " ".into() } else { text.into() });
        self.layout();
        self.flash_serial += 1;
        let serial = self.flash_serial;
        slint::Timer::single_shot(Duration::from_millis(ms), move || {
            with_app(move |app| {
                if app.flash_serial == serial {
                    app.ui.set_pill_flash("".into());
                    if app.ui.get_mode() == PILL {
                        app.layout();
                    }
                }
            })
        });
    }

    /// Largura extra da pílula para as bolinhas de câmera/microfone.
    fn privacy_dots(&self) -> f32 {
        let n = (!self.dev.cam.is_empty()) as i32 + (!self.dev.mic.is_empty()) as i32;
        n as f32 * 17.0
    }

    /// Novo estado do aparelho: atualiza o Início e avisa na pílula o que mudou.
    fn apply_devices(&mut self, new: DevState) {
        let old = std::mem::replace(&mut self.dev, new);
        let d = &self.dev;
        self.ui.set_cam_app(d.cam.join(", ").into());
        self.ui.set_mic_app(d.mic.join(", ").into());
        if let Some(b) = d.bat {
            self.ui.set_has_bat(true);
            self.ui.set_bat_pct(b.percent as i32);
            self.ui.set_bat_charging(b.charging);
        } else {
            self.ui.set_has_bat(false);
            self.ui.set_bat_charging(false);
        }
        match d.headsets.first() {
            Some(h) => {
                self.ui.set_hs_name(h.name.clone().into());
                self.ui.set_hs_pct(h.percent as i32);
            }
            None => {
                self.ui.set_hs_name("".into());
                self.ui.set_hs_pct(-1);
            }
        }
        let dots_changed = (old.cam.is_empty(), old.mic.is_empty()) != (self.dev.cam.is_empty(), self.dev.mic.is_empty());
        // Avisos (só depois da primeira leitura, para não avisar tudo ao abrir).
        if old.read {
            let (new_bat, new_hs) = (self.dev.bat, self.dev.headsets.clone());
            if let (Some(n), Some(o)) = (new_bat, old.bat) {
                if n.charging && !o.charging {
                    self.flash_pill("\u{E945}", "Carregando", Some([0x34, 0xc7, 0x59, 0xff]), &format!("{}%", n.percent), 2500);
                } else if !n.charging {
                    for limit in [20u8, 10] {
                        if n.percent <= limit && o.percent > limit {
                            self.flash_pill("\u{E850}", &format!("Bateria em {}%", n.percent), Some([0xff, 0x45, 0x3a, 0xff]), "ligue o carregador", 4000);
                        }
                    }
                }
            }
            for h in &new_hs {
                if !old.headsets.iter().any(|o| o.name == h.name) {
                    self.flash_pill("\u{E7F6}", &h.name, None, &format!("conectado · {}%", h.percent), 3000);
                }
            }
        }
        if dots_changed && self.ui.get_mode() == PILL {
            self.layout();
        }
        if self.ui.get_mode() == CALENDAR && self.page == 5 {
            self.refresh_home();
        }
    }

    /// Dados do Início: agenda de hoje e última mensagem.
    fn refresh_home(&self) {
        let (y, m, d, h, min, _) = win::local_time();
        self.ui.set_h_date(format::date_label(y, m, d).into());
        let today = iso_today();
        let now = format!("{h:02}:{min:02}");
        let events = self.cal.on(&today);
        let pending: Vec<_> = events.iter().filter(|e| !e.done && e.time.as_deref().is_none_or(|t| t >= now.as_str())).collect();
        match pending.first() {
            Some(e) => {
                let label = match &e.time {
                    Some(t) => format!("{t} · {}", e.title),
                    None => e.title.clone(),
                };
                self.ui.set_h_event(label.into());
                let more = pending.len() - 1;
                self.ui.set_h_event_more(match more {
                    0 => "Mais nada hoje".into(),
                    1 => "+1 evento hoje".into(),
                    n => format!("+{n} eventos hoje").into(),
                });
            }
            None => {
                self.ui.set_h_event("Nada marcado hoje".into());
                self.ui.set_h_event_more("Toque para ver a agenda".into());
            }
        }
        match &self.last_msg {
            Some((src, who, text)) => {
                self.ui.set_h_msg_src(src.clone().into());
                self.ui.set_h_msg_sender(who.clone().into());
                self.ui.set_h_msg_text(text.clone().into());
            }
            None => {
                self.ui.set_h_msg_src("Mensagens".into());
                self.ui.set_h_msg_sender("Nenhuma mensagem nova".into());
                self.ui.set_h_msg_text("WhatsApp e e-mail aparecem aqui".into());
            }
        }
    }

    fn refresh_clips(&mut self) {
        let now = std::time::SystemTime::now();
        let cards: Vec<ClipCard> = self
            .clips
            .iter()
            .map(|c| {
                let secs = now.duration_since(c.at).map(|d| d.as_secs()).unwrap_or(0);
                let (summary, swatch) = match &c.data {
                    clipboard::Data::Text(t) => {
                        let sw = clip::parse_color(t).map(|(r, g, b)| Color::from_rgb_u8(r, g, b)).unwrap_or_default();
                        (clip::summary(t), sw)
                    }
                    clipboard::Data::Files(f) => {
                        let first = f.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        (if f.len() > 1 { format!("{first} +{}", f.len() - 1) } else { first }, Color::default())
                    }
                    clipboard::Data::Image(w, h, _) => (format!("{w}×{h}"), Color::default()),
                };
                ClipCard {
                    kind: c.kind.label().into(),
                    icon: clip_icon(c.kind).into(),
                    summary: summary.into(),
                    age: clip::age(secs).into(),
                    pinned: c.pinned,
                    is_image: c.thumb.is_some(),
                    thumb: c.thumb.clone().unwrap_or_default(),
                    is_color: c.kind == clip::Kind::Color,
                    swatch,
                }
            })
            .collect();
        self.ui.set_clips(ModelRc::new(VecModel::from(cards)));
        self.ui.set_clip_open(self.clip_open.map(|i| i as i32).unwrap_or(-1));
    }

    /// Abre o preview de um cartão; a ilha muda de tamanho conforme o conteúdo.
    fn open_clip(&mut self, i: usize) {
        let Some(c) = self.clips.get(i) else { return };
        let secs = std::time::SystemTime::now().duration_since(c.at).map(|d| d.as_secs()).unwrap_or(0);
        let mut meta = format!("{} · {}", c.kind.label(), clip::age(secs));
        let mut can_open = false;
        let view = match &c.data {
            clipboard::Data::Image(w, h, _) => {
                self.ui.set_clip_image(c.thumb.clone().unwrap_or_default());
                meta = format!("{meta} · {w}×{h}");
                3
            }
            clipboard::Data::Files(f) => {
                let list: Vec<String> = f.iter().map(|p| p.display().to_string()).collect();
                self.ui.set_clip_text(list.join("\n").into());
                can_open = true;
                4
            }
            clipboard::Data::Text(t) => match c.kind {
                clip::Kind::Code => {
                    let lines = clip::highlight(t, 400);
                    let cols = lines.iter().map(|l| l.iter().map(|s| s.0.chars().count()).sum::<usize>()).max().unwrap_or(0);
                    let model: Vec<CodeLine> = lines
                        .into_iter()
                        .map(|l| CodeLine {
                            spans: ModelRc::new(VecModel::from(
                                l.into_iter()
                                    .map(|(text, tok)| {
                                        let (r, g, b) = tok.rgb();
                                        CodeSpan { text: text.into(), color: Color::from_rgb_u8(r, g, b) }
                                    })
                                    .collect::<Vec<_>>(),
                            )),
                        })
                        .collect();
                    self.ui.set_clip_code(ModelRc::new(VecModel::from(model)));
                    self.ui.set_clip_code_cols(cols as i32);
                    meta = format!("{meta} · {} linhas", t.lines().count());
                    2
                }
                clip::Kind::Link => {
                    self.ui.set_clip_text(t.trim().into());
                    can_open = true;
                    5
                }
                clip::Kind::Color => {
                    if let Some((r, g, b)) = clip::parse_color(t) {
                        self.ui.set_clip_swatch(Color::from_rgb_u8(r, g, b));
                        self.ui.set_clip_text(format!("{}\nrgb({r}, {g}, {b})", t.trim()).into());
                    }
                    6
                }
                _ => {
                    self.ui.set_clip_text(t.as_str().into());
                    if t.len() > 280 || t.lines().count() > 5 { 1 } else { 0 }
                }
            },
        };
        self.ui.set_clip_view(view);
        self.ui.set_clip_meta(meta.into());
        self.ui.set_clip_pinned(c.pinned);
        self.ui.set_clip_can_open(can_open);
        self.clip_open = Some(i);
        self.ui.set_clip_open(i as i32);
        self.layout();
    }

    fn close_clip(&mut self) {
        self.clip_open = None;
        self.refresh_clips();
        self.layout();
    }

    fn save_pinned_clips(&self) {
        let items: Vec<serde_json::Value> = self
            .clips
            .iter()
            .filter(|c| c.pinned)
            .filter_map(|c| match &c.data {
                clipboard::Data::Text(t) => Some(serde_json::json!({ "text": t })),
                clipboard::Data::Files(f) => Some(serde_json::json!({ "files": f })),
                clipboard::Data::Image(..) => None,
            })
            .collect();
        let _ = std::fs::write(self.dir.join("clipboard.json"), serde_json::to_vec_pretty(&items).unwrap_or_default());
    }

    /// Zera o "não lido" da aba de mensagens que está na tela.
    fn mark_read(&self) {
        if self.ui.get_msg_tab() == 1 {
            self.ui.set_mail_unread(0);
        } else {
            self.ui.set_wa_unread(0);
        }
    }

    /// Abre o WhatsApp ou o e-mail (Outlook, ou o Gmail no navegador).
    fn open_messages_app(&mut self, mail: bool) {
        if mail {
            let _ = nocto_core::launcher::open_url(nocto_core::notify::mail_target(self.mail_source));
        } else {
            let _ = std::process::Command::new("explorer.exe").arg("whatsapp:").spawn();
        }
        self.set_mode(PILL);
    }

    /// Atalhos que dá para escolher na página de configurações.
    const HOTKEYS: [&'static str; 4] = ["alt+shift+space", "ctrl+alt+n", "win+alt+space", "ctrl+shift+space"];

    fn refresh_settings(&self) {
        let apps: Vec<AppItem> = self
            .allow
            .sorted_names()
            .into_iter()
            .map(|name| {
                let icon = self.allow.get(&name).and_then(|target| self.app_icon(target));
                AppItem {
                    letter: name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
                    name: name.into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                }
            })
            .collect();
        self.ui.set_apps(ModelRc::new(VecModel::from(apps)));
        self.ui.set_hide_fullscreen(self.config.hide_in_fullscreen);
        self.ui.set_hotkey_label(self.config.hotkey.replace('+', " + ").into());
        self.ui.set_whatsapp_on(self.config.whatsapp);
        self.ui.set_whatsapp_preview(self.config.whatsapp_preview);
        self.ui.set_wake_on(self.config.wake_word);
        self.ui.set_autostart_on(self.config.autostart);
        self.ui.set_clip_flash_on(self.config.clip_flash);
        let own = !self.config.spotify_client_id.is_empty() && self.config.spotify_client_id != DEFAULT_SPOTIFY_CLIENT_ID;
        self.ui.set_spotify_own(own);
        self.ui.set_open_page_label(self.config.open_page.label().into());
        self.ui.set_glass_label(self.config.glass.label().into());
    }

    /// Ícone do app (do .exe ou atalho), lido uma vez e guardado.
    fn app_icon(&self, target: &str) -> Option<slint::Image> {
        if let Some(cached) = self.icons.borrow().get(target) {
            return cached.clone();
        }
        let icon = win::icon_source(target).and_then(|p| win::file_icon_rgba(&p)).map(|(w, h, px)| {
            slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&px, w, h))
        });
        self.icons.borrow_mut().insert(target.to_string(), icon.clone());
        icon
    }

    fn save_allowlist(&self) {
        let _ = std::fs::write(self.dir.join("allowlist.json"), self.allow.to_json());
    }

    /// Mostra a página de configurações (abre o painel se estiver fechado).
    fn show_settings(&mut self) {
        self.ui.set_slide(self.ui.get_mode() == CALENDAR);
        self.page = 4;
        self.ui.set_page(4);
        self.refresh_settings();
        if self.ui.get_mode() == CALENDAR {
            self.layout();
        } else {
            self.apply_theme();
            self.set_mode(CALENDAR);
        }
    }

    /// App arrastado até a ilha: entra na lista do "abre".
    fn add_app(&mut self, name: String, path: PathBuf) {
        self.allow.add(&name, &path.to_string_lossy());
        self.save_allowlist();
        self.ui.set_drop_target(false);
        let mut shown = name.clone();
        if let Some(first) = shown.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        self.ui.set_settings_note(format!("✓ {shown} adicionado. Digite: abre {name}").into());
        self.show_settings();
    }

    /// Abre o painel: na música se houver uma; senão, no calendário.
    fn open_panel(&mut self, page: i32) {
        self.ui.set_slide(false);
        self.page = page;
        self.ui.set_page(page);
        if page == 2 {
            // Abre na aba que tem mensagem nova (WhatsApp primeiro).
            if self.ui.get_wa_unread() == 0 && self.ui.get_mail_unread() > 0 {
                self.ui.set_msg_tab(1);
            } else if self.ui.get_wa_unread() > 0 {
                self.ui.set_msg_tab(0);
            }
            self.mark_read();
        }
        if page == 3 {
            self.refresh_clips();
        }
        if page == 5 {
            self.refresh_home();
        }
        self.ui.set_settings_note("".into());
        self.apply_theme();
        self.set_mode(CALENDAR);
        win::activate(self.hwnd);
        if page == 1 {
            self.ui.invoke_focus_item_input();
        } else {
            self.ui.invoke_focus_panel();
        }
    }

    /// Lê o volume do Spotify no mixer do Windows e mostra no knob.
    fn refresh_volume(&self) {
        self.volume.get(|v| {
            on_ui(move |app| {
                app.ui.set_has_volume(v.is_some());
                if let Some(v) = v {
                    app.ui.set_volume(v);
                }
            })
        });
    }

    /// A barra de tempo só anda com a página de música aberta e a música tocando.
    fn update_progress_timer(&self) {
        self.refresh_progress();
        let run = self.ui.get_mode() == CALENDAR && (self.page == 0 || self.page == 5) && self.ui.get_playing();
        if run && !self.progress_timer.running() {
            self.progress_timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
                with_app(|app| app.refresh_progress())
            });
        } else if !run {
            self.progress_timer.stop();
        }
        let run_k = self.karaoke && self.ui.get_mode() == CALENDAR && self.page == 0;
        if run_k && !self.karaoke_timer.running() {
            self.karaoke_timer.start(slint::TimerMode::Repeated, Duration::from_millis(50), || {
                with_app(|app| app.karaoke_tick())
            });
        } else if !run_k {
            self.karaoke_timer.stop();
        }
    }

    fn toggle_karaoke(&mut self) {
        self.karaoke = !self.karaoke;
        self.ui.set_karaoke(self.karaoke);
        if self.karaoke {
            self.load_lyrics();
            self.karaoke_tick();
        }
        self.layout();
        self.update_progress_timer();
    }

    /// Busca a letra da música atual (numa thread; o LRCLIB pode demorar).
    fn load_lyrics(&mut self) {
        let Some(t) = &self.track else {
            self.lyrics_key.clear();
            self.lyrics = None;
            self.ui.set_k_msg("Nada tocando".into());
            return;
        };
        let key = format!("{}\u{1}{}", t.artist, t.title);
        if key == self.lyrics_key {
            return;
        }
        self.lyrics_key = key.clone();
        self.lyrics = None;
        self.ui.set_k_msg("Procurando a letra…".into());
        let (dir, artist, title, dur) = (self.dir.clone(), t.artist.clone(), t.title.clone(), t.duration_ms / 1000);
        std::thread::spawn(move || {
            let res = nocto_core::lyrics::fetch(&dir, &artist, &title, dur);
            on_ui(move |app| {
                if app.lyrics_key != key {
                    return;
                }
                let msg = match res {
                    Ok(Some(l)) => {
                        app.lyrics = Some(l);
                        ""
                    }
                    Ok(None) => "Sem letra para esta música",
                    Err(_) => "Sem conexão para buscar a letra",
                };
                app.ui.set_k_msg(msg.into());
                app.karaoke_tick();
            });
        });
    }

    /// Spotify conectado: acerta a posição pela API a cada ~2 s (a do Windows atrasa).
    fn karaoke_sync(&mut self) {
        let Some(t) = &self.track else { return };
        if !t.spotify {
            return;
        }
        let title = t.title.clone();
        let cfg = self.spotify_config();
        std::thread::spawn(move || {
            let sent = std::time::Instant::now();
            let Some((name, pos, playing)) = spotify::now_playing(&cfg) else { return };
            let half = sent.elapsed() / 2;
            on_ui(move |app| {
                if let Some(t) = app.track.as_mut() {
                    if t.title == name || name.starts_with(&t.title) || t.title.starts_with(&name) {
                        t.position_ms = pos + half.as_millis() as u64;
                        t.read_at = sent + half;
                        app.synced_at = Some(std::time::Instant::now());
                        t.playing = playing;
                    }
                }
                let _ = title;
            });
        });
    }

    fn karaoke_tick(&mut self) {
        self.sync_tick = self.sync_tick.wrapping_add(1);
        if self.sync_tick % 40 == 1 {
            self.karaoke_sync();
        }
        let (Some(t), Some(l)) = (&self.track, &self.lyrics) else {
            for f in [Island::set_k_prev, Island::set_k_cur, Island::set_k_next, Island::set_k_next2] {
                f(&self.ui, "".into());
            }
            self.ui.set_k_gap(false);
            return;
        };
        let target = format::position_now(t.position_ms, t.read_at.elapsed().as_millis() as u64, t.playing, t.duration_ms) as f64;
        // Relógio suave: anda com o tempo real e corrige devagar em direção à posição
        // informada, sem voltar para trás (só pula se a diferença for grande, como num seek).
        let now = std::time::Instant::now();
        let dt = self.k_clock.map_or(0.0, |c| now.duration_since(c).as_secs_f64() * 1000.0);
        self.k_clock = Some(now);
        if t.playing {
            self.k_pos += dt;
        }
        let err = target - self.k_pos;
        if err.abs() > 1500.0 || !t.playing {
            self.k_pos = target;
        } else {
            self.k_pos += (err * 0.08).max(-dt * 0.5);
        }
        let pos = self.k_pos.max(0.0) as u64;
        let lines = &l.lines;
        let text = |i: Option<usize>| -> slint::SharedString {
            i.and_then(|i| lines.get(i)).map(|x| x.1.as_str()).unwrap_or("").into()
        };
        let (idx, fill, gap) = if l.synced {
            let idx = nocto_core::lyrics::current(lines, pos);
            let start = idx.map(|i| lines[i].0).unwrap_or(0);
            let end = idx.map_or(lines.first().map(|x| x.0), |i| lines.get(i + 1).map(|x| x.0)).unwrap_or(t.duration_ms.max(start + 4000));
            let fill = idx.map_or(0.0, |i| l.fill(i, pos));
            // Linha vazia, ou a linha já acabou e a próxima demora: bolinhas.
            let empty = idx.map_or(true, |i| lines[i].1.is_empty() || l.ends[i].is_some_and(|e| pos > e + 2500));
            let gap = empty && end.saturating_sub(pos) > 1500;
            (idx, fill, gap)
        } else {
            // Sem sincronia: rola a letra conforme o andamento da música.
            let n = lines.len().max(1);
            let i = if t.duration_ms > 0 { (pos as usize * n / t.duration_ms as usize).min(n - 1) } else { 0 };
            (Some(i), 0.0, false)
        };
        let i = idx.map(|i| i as isize).unwrap_or(-1);
        let at = |k: isize| if i + k >= 0 { Some((i + k) as usize) } else { None };
        if idx != self.k_idx {
            // Linha nova: o bloco aparece uma linha abaixo e sobe até o lugar.
            if self.k_idx.is_some() && idx.is_some() && idx > self.k_idx {
                self.ui.set_k_jump(true);
                self.ui.set_k_shift(30.0);
                slint::Timer::single_shot(Duration::from_millis(16), || {
                    with_app(|app| {
                        app.ui.set_k_jump(false);
                        app.ui.set_k_shift(0.0);
                    })
                });
            }
            self.k_idx = idx;
        }
        self.ui.set_k_prev(text(at(-1)));
        self.ui.set_k_cur(text(at(0)));
        self.ui.set_k_next(text(at(1)));
        self.ui.set_k_next2(text(at(2)));
        self.ui.set_k_fill(fill);
        self.ui.set_k_gap(gap);
        self.ui.set_progress(format::progress(pos, t.duration_ms));
    }

    fn refresh_progress(&self) {
        let Some(t) = &self.track else {
            self.ui.set_progress(0.0);
            self.ui.set_pos_text("0:00".into());
            self.ui.set_dur_text("0:00".into());
            return;
        };
        let elapsed = t.read_at.elapsed().as_millis() as u64;
        let pos = format::position_now(t.position_ms, elapsed, t.playing, t.duration_ms);
        self.ui.set_progress(format::progress(pos, t.duration_ms));
        self.ui.set_pos_text(format::mmss(pos).into());
        self.ui.set_dur_text(format::mmss(t.duration_ms).into());
    }

    /// Música ou relógio na pílula, conforme a escolha (automático, música ou relógio).
    fn update_compact(&self) {
        let show = self.config.compact.shows_music(self.ui.get_has_track(), self.ui.get_playing());
        self.ui.set_compact_label(self.config.compact.label().into());
        self.ui.set_compact_short(
            match self.config.compact {
                CompactMode::Auto => "Automático",
                CompactMode::Music => "Música",
                CompactMode::Clock => "Relógio",
            }
            .into(),
        );
        if show != self.ui.get_show_music() {
            self.ui.set_show_music(show);
            if self.ui.get_mode() == PILL {
                self.layout();
            }
        }
    }

    /// Atualiza o relógio e agenda a próxima atualização para a virada do minuto.
    fn refresh_clock(&self) {
        let (y, m, d, h, min, s) = win::local_time();
        self.ui.set_clock(format::clock(h, min).into());
        let date = if self.pill_note.is_empty() { format::date_label(y, m, d) } else { self.pill_note.clone() };
        self.ui.set_date_text(date.into());
        slint::Timer::single_shot(Duration::from_secs((60 - s.min(59)) as u64), || {
            with_app(|app| app.refresh_clock())
        });
    }

    fn set_status(&self, text: &str, error: bool) {
        self.ui.set_status(text.into());
        self.ui.set_status_error(error);
        self.layout();
    }

    fn apply_theme(&self) {
        let (_, _, _, h, m, _) = win::local_time();
        let p = theme::palette(self.config.theme.resolve(h, m));
        self.ui.set_glass(color(p.glass));
        if let Some(glass) = &self.glass {
            let [r, g, b, a] = p.glass;
            let (blur, tint) = self.config.glass.levels();
            let _ = glass.set_blur(blur);
            let _ = glass.set_tint([(a as f32 * tint) as u8, r, g, b], 3000);
        }
        self.ui.set_accent(color(p.accent));
        self.ui.set_ink(color(p.text));
        self.ui.set_k_color(color(karaoke_rgb(&self.config.karaoke_color, self.config.theme.resolve(h, m))));
        self.ui.set_k_color_label(karaoke_label(&self.config.karaoke_color).into());
        self.ui.set_theme_label(self.config.theme.label().into());
    }

    /// Liga/desliga a escuta do "Nocto" conforme a configuração.
    fn apply_wake(&mut self) {
        if !self.config.wake_word {
            self.wake = None; // solta o microfone e o modelo
            return;
        }
        if self.wake.is_none() {
            let paused = self.wake_paused.clone();
            self.wake = Some(wake::start(self.dir.clone(), paused, || {
                on_ui(|app| {
                    if app.ui.get_listening() || app.fullscreen_app {
                        return;
                    }
                    if app.ui.get_mode() != BAR {
                        app.toggle_bar();
                    }
                    app.ui.invoke_voice();
                })
            }));
        }
        self.update_wake_pause();
    }

    /// Escuta do "Nocto" pausa em jogo de tela cheia e enquanto já está ouvindo um comando.
    fn update_wake_pause(&self) {
        let paused = self.fullscreen_app || self.ui.get_listening();
        self.wake_paused.store(paused, std::sync::atomic::Ordering::Relaxed);
    }

    fn toggle_bar(&mut self) {
        if self.ui.get_mode() == BAR {
            self.close(true);
            return;
        }
        let fg = win::foreground();
        if fg != self.hwnd {
            self.prev_foreground = fg;
        }
        self.apply_theme();
        self.ui.set_status("".into());
        self.set_mode(BAR);
        win::activate(self.hwnd);
        self.ui.invoke_focus_input();
    }

    /// Volta à pílula; `restore` devolve o foco ao app que estava em uso.
    fn close(&mut self, restore: bool) {
        self.dropped = None;
        self.ui.set_drop_name("".into());
        self.ui.set_status("".into());
        self.set_mode(PILL);
        if restore && self.prev_foreground != 0 {
            win::activate(self.prev_foreground);
        }
    }

    fn spotify_config(&mut self) -> spotify::Config {
        // Client ID vazio na memória: relê o arquivo (pode ter sido editado à mão).
        if self.config.spotify_client_id.is_empty() {
            let disk = Config::load(&self.dir.join("config.json"));
            self.config.spotify_client_id = disk.spotify_client_id;
        }
        // Último recurso: o Client ID do app do Nocto (não é segredo; o login usa PKCE).
        if self.config.spotify_client_id.is_empty() {
            self.config.spotify_client_id = DEFAULT_SPOTIFY_CLIENT_ID.into();
        }
        spotify::Config {
            client_id: self.config.spotify_client_id.clone(),
            token_path: self.dir.join("spotify-token.json"),
        }
    }

    fn submit(&mut self, text: &str) {
        match commands::parse(text) {
            Command::Ignore => {}
            cmd @ (Command::OpenApp(_) | Command::OpenUrl(_) | Command::OpenFolder(_)) => {
                match launcher::run(&cmd, &self.allow) {
                    Ok(()) => self.close(false),
                    Err(e) => {
                        let hint = if matches!(cmd, Command::OpenApp(_)) {
                            " (arraste o app até a ilha para adicionar)"
                        } else {
                            ""
                        };
                        self.set_status(&format!("{e}{hint}"), true);
                    }
                }
            }
            Command::SpotifyPlay(q) => {
                self.set_status(&format!("Procurando \"{q}\" no Spotify…"), false);
                let cfg = self.spotify_config();
                std::thread::spawn(move || {
                    let result = spotify::play_query(&cfg, &q);
                    on_ui(move |app| match result {
                        Ok(msg) => {
                            app.set_status(&msg, false);
                            slint::Timer::single_shot(Duration::from_millis(1500), || {
                                with_app(|app| {
                                    if app.ui.get_mode() == BAR {
                                        app.close(true);
                                    }
                                })
                            });
                        }
                        Err(e) => app.set_status(&e, true),
                    });
                });
            }
            Command::SpotifyPlaylist(name) => {
                self.set_status(&format!("Procurando a playlist \"{name}\"…"), false);
                let cfg = self.spotify_config();
                std::thread::spawn(move || {
                    let result = spotify::play_playlist(&cfg, &name);
                    on_ui(move |app| match result {
                        Ok(msg) => {
                            app.set_status(&msg, false);
                            slint::Timer::single_shot(Duration::from_millis(1500), || {
                                with_app(|app| {
                                    if app.ui.get_mode() == BAR {
                                        app.close(true);
                                    }
                                })
                            });
                        }
                        Err(e) => app.set_status(&e, true),
                    });
                });
            }
            Command::SpotifyQueue(name) => {
                self.set_status(&format!("Procurando \"{name}\"…"), false);
                let cfg = self.spotify_config();
                std::thread::spawn(move || {
                    let result = spotify::queue_query(&cfg, &name);
                    on_ui(move |app| match result {
                        Ok(msg) => {
                            app.set_status(&msg, false);
                            slint::Timer::single_shot(Duration::from_millis(1500), || {
                                with_app(|app| {
                                    if app.ui.get_mode() == BAR {
                                        app.close(true);
                                    }
                                })
                            });
                        }
                        Err(e) => app.set_status(&e, true),
                    });
                });
            }
            Command::SpotifyAlbum(name) => {
                self.set_status(&format!("Procurando o álbum \"{name}\"…"), false);
                let cfg = self.spotify_config();
                std::thread::spawn(move || {
                    let result = spotify::play_album(&cfg, &name);
                    on_ui(move |app| match result {
                        Ok(msg) => {
                            app.set_status(&msg, false);
                            slint::Timer::single_shot(Duration::from_millis(1500), || {
                                with_app(|app| {
                                    if app.ui.get_mode() == BAR {
                                        app.close(true);
                                    }
                                })
                            });
                        }
                        Err(e) => app.set_status(&e, true),
                    });
                });
            }
            Command::SpotifyPause => {
                media::play_pause();
                self.close(true);
            }
            Command::SpotifyNext => {
                media::next();
                self.close(true);
            }
            Command::SpotifyLogin => {
                self.set_status("Abrindo o login do Spotify no navegador…", false);
                let cfg = self.spotify_config();
                std::thread::spawn(move || {
                    let result = spotify::login(&cfg);
                    on_ui(move |app| match result {
                        Ok(()) => app.set_status("Spotify conectado. Tente: toca <música>", false),
                        Err(e) => app.set_status(&e, true),
                    });
                });
            }
            Command::ShowCalendar => self.open_panel(1),
            Command::ShowClipboard => self.open_panel(3),
            Command::SpotifyClientId(id) => {
                self.config.spotify_client_id = id;
                let _ = self.config.save(&self.dir.join("config.json"));
                // O login antigo era de outro app: começa do zero com o novo.
                let _ = std::fs::remove_file(self.dir.join("spotify-token.json"));
                self.refresh_settings();
                self.set_status("✓ Client ID salvo. Agora digite: conectar spotify", false);
            }
            Command::Timer(nocto_core::timer::TimerCmd::Start(secs)) => {
                self.start_timer(secs);
                self.close(false);
            }
            Command::Timer(nocto_core::timer::TimerCmd::Cancel) => {
                if self.timer_end.is_some() {
                    self.stop_timer();
                    self.close(false);
                } else {
                    self.set_status("Não tem timer rodando.", false);
                }
            }
            Command::Quit => {
                let _ = slint::quit_event_loop();
            }
            Command::Unknown(_) => self.set_status(
                "Não conheço esse comando. Tente: abre spotify · toca <música> · agenda · pausa · próxima",
                true,
            ),
        }
    }

    fn refresh_calendar(&self) {
        let (y, m) = self.view;
        let today = iso_today();
        let days: Vec<DayCell> = calendar::month_grid(y, m)
            .iter()
            .map(|d| {
                let iso = d.iso();
                DayCell {
                    label: d.day.to_string().into(),
                    in_month: d.in_month,
                    today: iso == today,
                    selected: iso == self.selected,
                    marked: !self.cal.on(&iso).is_empty(),
                    iso: iso.into(),
                }
            })
            .collect();
        let items: Vec<Item> = self
            .cal
            .on(&self.selected)
            .iter()
            .map(|e| Item {
                time: e.time.clone().unwrap_or_default().into(),
                title: e.title.clone().into(),
                done: e.done,
            })
            .collect();
        let month = MONTHS[(m - 1) as usize];
        let mut label = month.to_string();
        label[..1].make_ascii_uppercase();
        self.ui.set_month_label(format!("{label} {y}").into());
        self.ui.set_selected_label(self.selected_label().into());
        self.ui.set_days(ModelRc::new(VecModel::from(days)));
        self.ui.set_items(ModelRc::new(VecModel::from(items)));
    }

    fn selected_label(&self) -> String {
        let parts: Vec<u32> = self.selected.split('-').filter_map(|p| p.parse().ok()).collect();
        let [y, m, d] = parts[..] else { return self.selected.clone() };
        let wd = WEEKDAYS[calendar::weekday(y as i32, m, d) as usize];
        format!("{wd}, {d} de {}", MONTHS[(m - 1) as usize])
    }

    fn save_calendar(&self) {
        if let Err(e) = self.cal.save(&self.dir.join("calendar.json")) {
            self.ui.set_selected_label(format!("Erro ao salvar: {e}").into());
        }
    }

    fn shift_month(&mut self, delta: i32) {
        let (y, m) = self.view;
        let idx = y * 12 + (m as i32 - 1) + delta;
        self.view = (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32);
        self.refresh_calendar();
    }

    /// Miniatura de uma imagem na tela de soltar arquivo (lida fora da interface).
    fn show_drop_preview(&self, path: &std::path::Path, label: &'static str) {
        let path = path.to_path_buf();
        self.preview_path.replace(Some(path.clone()));
        std::thread::spawn(move || {
            let Some((w, h, px)) = std::fs::read(&path).ok().and_then(|b| imgconv::decode_rgba(&b, 232)) else { return };
            on_ui(move |app| {
                let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&px, w, h);
                app.ui.set_drop_preview(slint::Image::from_rgba8(buf));
                app.ui.set_drop_preview_label(label.into());
                if !app.ui.get_has_drop_preview() {
                    app.ui.set_has_drop_preview(true);
                    if app.ui.get_mode() == DROP {
                        app.layout();
                    }
                }
            });
        });
    }

    fn convert(&mut self, fmt: &str) {
        let Some(src) = self.dropped.clone() else { return };
        if fmt == "nobg" {
            let dir = self.dir.clone();
            let first = !dir.join("silueta.onnx").exists();
            self.set_status(if first { "Baixando o modelo (44 MB, só na primeira vez)…" } else { "Removendo o fundo… (uns 10 s)" }, false);
            std::thread::spawn(move || {
                let result = nocto_core::bgremove::ensure_model(&dir).and_then(|model| {
                    if first {
                        on_ui(|app| app.set_status("Removendo o fundo… (uns 10 s)", false));
                    }
                    nocto_core::bgremove::remove_background(&src, &model)
                });
                on_ui(move |app| match result {
                    Ok(out) => {
                        let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        app.show_drop_preview(&out, "Resultado");
                        app.set_status(&format!("Salvo ao lado do original: {name}"), false);
                    }
                    Err(e) => app.set_status(&e, true),
                });
            });
            return;
        }
        let fmt = match fmt {
            "jpeg" => Format::Jpeg,
            "webp" => Format::WebP,
            _ => Format::Png,
        };
        self.set_status("Convertendo…", false);
        std::thread::spawn(move || {
            let result = imgconv::convert_image(&src, fmt);
            on_ui(move |app| match result {
                Ok(out) => {
                    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    app.show_drop_preview(&out, "Resultado");
                    app.set_status(&format!("Salvo ao lado do original: {name}"), false);
                }
                Err(e) => app.set_status(&e, true),
            });
        });
    }

    fn foreground_changed(&mut self, fg: isize) {
        let full = self.config.hide_in_fullscreen && win::is_fullscreen(fg);
        if full != self.fullscreen_app {
            self.fullscreen_app = full;
            self.update_wake_pause();
            if self.ui.get_mode() == PILL {
                self.layout();
            }
        }
        self.apply_theme();
    }
}

/// Um item da área de transferência.
struct Clip {
    data: clipboard::Data,
    kind: clip::Kind,
    at: std::time::SystemTime,
    pinned: bool,
    /// Imagem já reduzida para o cartão e o preview.
    thumb: Option<slint::Image>,
}

fn clip_icon(kind: clip::Kind) -> &'static str {
    match kind {
        clip::Kind::Text => "\u{E8A5}",
        clip::Kind::Link => "\u{E71B}",
        clip::Kind::Code => "\u{E943}",
        clip::Kind::Color => "\u{E790}",
        clip::Kind::Image => "\u{EB9F}",
        clip::Kind::File => "\u{E8B7}",
    }
}

/// Itens fixados que sobrevivem ao reiniciar (texto e arquivos).
fn load_pinned_clips(dir: &std::path::Path) -> Vec<Clip> {
    let Ok(text) = std::fs::read_to_string(dir.join("clipboard.json")) else { return Vec::new() };
    let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else { return Vec::new() };
    items
        .into_iter()
        .filter_map(|v| {
            let data = if let Some(t) = v["text"].as_str() {
                clipboard::Data::Text(t.to_string())
            } else {
                let files: Vec<PathBuf> = v["files"].as_array()?.iter().filter_map(|f| f.as_str().map(PathBuf::from)).collect();
                clipboard::Data::Files(files)
            };
            let kind = match &data {
                clipboard::Data::Text(t) => clip::classify_text(t),
                _ => clip::Kind::File,
            };
            Some(Clip { data, kind, at: std::time::SystemTime::now(), pinned: true, thumb: None })
        })
        .collect()
}

fn data_dir() -> PathBuf {
    // Pasta pedida ao próprio Windows (não à variável APPDATA, que pode faltar quando o
    // Nocto abre junto com o computador); a variável fica só de reserva.
    let base = win::roaming_appdata()
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("Nocto");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn load_allowlist(dir: &std::path::Path) -> Allowlist {
    let path = dir.join("allowlist.json");
    if !path.exists() {
        let _ = std::fs::write(&path, config::default_allowlist_json());
    }
    Allowlist::load(&path)
}

fn wire_callbacks(ui: &Island) {
    ui.on_submit(|text| {
        let text = text.to_string();
        with_app(move |app| app.submit(&text));
    });
    ui.on_cancel(|| with_app(|app| app.close(true)));
    ui.on_collapse(|| with_app(|app| app.close(false)));
    ui.on_expand(|| {
        with_app(|app| {
            // Mensagem não lida > música tocando > agenda.
            let unread = app.ui.get_wa_unread() + app.ui.get_mail_unread() > 0;
            let page = app.config.open_page.page(unread, app.ui.get_playing());
            app.open_panel(page);
        })
    });
    ui.on_prev_track(media::previous);
    ui.on_go_page(|page| with_app(move |app| app.go_page(page)));
    ui.on_remove_app(|name| {
        let name = name.to_string();
        with_app(move |app| {
            app.allow.remove(&name);
            app.save_allowlist();
            app.ui.set_settings_note(format!("{name} removido").into());
            app.refresh_settings();
        })
    });
    ui.on_toggle_fullscreen(|| {
        with_app(|app| {
            app.config.hide_in_fullscreen = !app.config.hide_in_fullscreen;
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
            app.foreground_changed(win::foreground());
        })
    });
    ui.on_voice(|| {
        with_app(|app| {
            if app.ui.get_listening() {
                return;
            }
            app.ui.set_listening(true);
            app.update_wake_pause();
            app.set_status("", false);
            let dir = app.dir.clone();
            let apps: Vec<String> = app.allow.names().map(|n| n.to_string()).collect();
            voice::listen(
                dir,
                apps,
                |msg| on_ui(move |app| app.set_status(msg, false)),
                |result| {
                    on_ui(move |app| {
                        app.ui.set_listening(false);
                        app.update_wake_pause();
                        match result {
                            Ok(text) => app.submit(&text),
                            Err(e) => app.set_status(&e, true),
                        }
                        // Sem foco (chamada por voz), a barra não fecharia sozinha: fecha em 5 s
                        // se ninguém mexer nela.
                        if app.ui.get_mode() == BAR && win::foreground() != app.hwnd {
                            slint::Timer::single_shot(Duration::from_secs(5), || {
                                with_app(|app| {
                                    if app.ui.get_mode() == BAR && !app.ui.get_listening() && win::foreground() != app.hwnd {
                                        app.close(false);
                                    }
                                })
                            });
                        }
                    })
                },
            );
        })
    });
    ui.on_open_preview(|| {
        with_app(|app| {
            if let Some(path) = app.preview_path.borrow().clone() {
                // "/select," abre a pasta já com o arquivo selecionado.
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
            }
        })
    });
    ui.on_open_notified(|| {
        with_app(|app| {
            if app.ui.get_notify_timer() {
                app.set_mode(PILL); // aviso do timer: o clique só fecha
            } else {
                app.open_messages_app(app.notify_source != Source::WhatsApp)
            }
        })
    });
    ui.on_open_messages(|mail| with_app(move |app| app.open_messages_app(mail)));
    ui.on_msg_tab_changed(|| with_app(|app| app.mark_read()));
    ui.on_toggle_whatsapp(|| {
        with_app(|app| {
            app.config.whatsapp = !app.config.whatsapp;
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
        })
    });
    ui.on_setup_spotify(|| {
        with_app(|app| {
            let _ = launcher::open_url("https://developer.spotify.com/dashboard");
            app.ui.set_settings_note(
                "1) Create app  2) Redirect URI: http://127.0.0.1:8898/callback  3) marque Web API  4) copie o Client ID e digite na barra: spotify id <código>".into(),
            );
        })
    });
    ui.on_clip_select(|i| with_app(move |app| app.open_clip(i as usize)));
    ui.on_clip_back(|| with_app(|app| app.close_clip()));
    ui.on_clip_copy(|| {
        with_app(|app| {
            let Some(c) = app.clip_open.and_then(|i| app.clips.get(i)) else { return };
            if clipboard::copy(&c.data) {
                app.ui.set_clip_meta("✓ Copiado. Cole com Ctrl + V".into());
            }
        })
    });
    ui.on_clip_open_item(|| {
        with_app(|app| {
            let Some(c) = app.clip_open.and_then(|i| app.clips.get(i)) else { return };
            match &c.data {
                clipboard::Data::Files(f) => {
                    if let Some(p) = f.first() {
                        let _ = launcher::open_url(&p.to_string_lossy());
                    }
                }
                clipboard::Data::Text(t) => {
                    let t = t.trim();
                    let url = if t.starts_with("www.") { format!("https://{t}") } else { t.to_string() };
                    let _ = launcher::open_url(&url);
                }
                _ => {}
            }
        })
    });
    ui.on_clip_pin(|| {
        with_app(|app| {
            let Some(i) = app.clip_open else { return };
            if let Some(c) = app.clips.get_mut(i) {
                c.pinned = !c.pinned;
                app.ui.set_clip_pinned(c.pinned);
            }
            app.save_pinned_clips();
        })
    });
    ui.on_clip_delete(|| {
        with_app(|app| {
            let Some(i) = app.clip_open else { return };
            if i < app.clips.len() {
                app.clips.remove(i);
            }
            app.save_pinned_clips();
            app.close_clip();
        })
    });
    ui.on_clip_clear(|| {
        with_app(|app| {
            app.clips.retain(|c| c.pinned);
            app.close_clip();
        })
    });
    ui.on_toggle_clip_flash(|| {
        with_app(|app| {
            app.config.clip_flash = !app.config.clip_flash;
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
        })
    });
    ui.on_toggle_autostart(|| {
        with_app(|app| {
            app.config.autostart = !app.config.autostart;
            let _ = app.config.save(&app.dir.join("config.json"));
            win::set_autostart(app.config.autostart);
            app.refresh_settings();
        })
    });
    ui.on_toggle_wake(|| {
        with_app(|app| {
            app.config.wake_word = !app.config.wake_word;
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
            app.apply_wake();
        })
    });
    ui.on_toggle_whatsapp_preview(|| {
        with_app(|app| {
            app.config.whatsapp_preview = !app.config.whatsapp_preview;
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
        })
    });
    ui.on_cycle_hotkey(|| {
        with_app(|app| {
            let i = App::HOTKEYS.iter().position(|h| *h == app.config.hotkey).map_or(0, |i| i + 1);
            app.config.hotkey = App::HOTKEYS[i % App::HOTKEYS.len()].to_string();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.ui.set_settings_note("O atalho novo vale ao reabrir o Nocto.".into());
            app.refresh_settings();
        })
    });
    ui.on_volume_changed(|v| {
        // Desenvolvimento: NOCTO_DEBUG registra cada mudança de volume.
        if std::env::var_os("NOCTO_DEBUG").is_some() {
            if let Some(dir) = std::env::var_os("APPDATA") {
                use std::io::Write;
                let path = std::path::Path::new(&dir).join("Nocto").join("debug.log");
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(f, "volume-changed {v:.3}");
                }
            }
        }
        with_app(move |app| app.volume.set(v))
    });
    ui.on_cycle_karaoke_color(|| {
        with_app(|app| {
            app.config.karaoke_color = next_karaoke_color(&app.config.karaoke_color).into();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.apply_theme();
        })
    });
    ui.on_cycle_open_page(|| {
        with_app(|app| {
            app.config.open_page = app.config.open_page.next();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
        })
    });
    ui.on_cycle_glass(|| {
        with_app(|app| {
            app.config.glass = app.config.glass.next();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.refresh_settings();
            app.apply_theme();
        })
    });
    ui.on_cycle_compact(|| {
        with_app(|app| {
            app.config.compact = app.config.compact.next();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.update_compact();
        })
    });
    ui.on_play_pause(media::play_pause);
    ui.on_toggle_karaoke(|| with_app(|app| app.toggle_karaoke()));
    ui.on_next_track(media::next);
    ui.on_prev_month(|| with_app(|app| app.shift_month(-1)));
    ui.on_next_month(|| with_app(|app| app.shift_month(1)));
    ui.on_pick_day(|iso| {
        let iso = iso.to_string();
        with_app(move |app| {
            app.selected = iso;
            app.refresh_calendar();
            app.ui.invoke_focus_item_input();
        })
    });
    ui.on_add_item(|text| {
        let text = text.to_string();
        with_app(move |app| {
            if let Some(entry) = calendar::parse_entry(&app.selected, &text) {
                app.cal.add(entry);
                app.save_calendar();
                app.refresh_calendar();
            }
        })
    });
    ui.on_toggle_item(|i| {
        with_app(move |app| {
            let date = app.selected.clone();
            app.cal.toggle(&date, i as usize);
            app.save_calendar();
            app.refresh_calendar();
        })
    });
    ui.on_remove_item(|i| {
        with_app(move |app| {
            let date = app.selected.clone();
            app.cal.remove(&date, i as usize);
            app.save_calendar();
            app.refresh_calendar();
        })
    });
    ui.on_cycle_theme(|| {
        with_app(|app| {
            app.config.theme = app.config.theme.next();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.apply_theme();
        })
    });
    ui.on_convert(|fmt| {
        let fmt = fmt.to_string();
        with_app(move |app| app.convert(&fmt));
    });

    // Arrastar e soltar arquivos e perda de foco vêm direto dos eventos da janela.
    ui.window().on_winit_window_event(|_, event| {
        match event {
            // Arrastar um app (.exe ou atalho) abre as configurações para adicioná-lo.
            WindowEvent::HoveredFile(path) if launcher::app_name(path).is_some() => with_app(|app| {
                app.ui.set_drop_target(true);
                app.ui.set_settings_note("".into());
                app.show_settings();
            }),
            WindowEvent::DroppedFile(path) if launcher::app_name(path).is_some() => {
                let path = path.clone();
                let name = launcher::app_name(&path).unwrap_or_default();
                with_app(move |app| app.add_app(name, path))
            }
            WindowEvent::HoveredFile(_) => with_app(|app| {
                if app.ui.get_mode() != DROP {
                    app.dropped = None;
                    app.ui.set_has_drop_preview(false);
                    app.ui.set_drop_name("".into());
                    app.ui.set_status("".into());
                    app.set_mode(DROP);
                }
            }),
            WindowEvent::DroppedFile(path) => {
                let path = path.clone();
                with_app(move |app| {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    app.ui.set_drop_name(name.into());
                    app.show_drop_preview(&path, "Original");
                    app.dropped = Some(path);
                    app.ui.set_status("Converter para:".into());
                    app.ui.set_status_error(false);
                    app.set_mode(DROP);
                })
            }
            WindowEvent::HoveredFileCancelled => with_app(|app| {
                if app.ui.get_drop_target() {
                    app.ui.set_drop_target(false);
                    return;
                }
                if app.dropped.is_none() {
                    app.close(false);
                }
            }),
            WindowEvent::Focused(_) | WindowEvent::Resized(_) | WindowEvent::Occluded(false) | WindowEvent::ScaleFactorChanged { .. } => {
                let lost_focus = matches!(event, WindowEvent::Focused(false));
                let got_focus = matches!(event, WindowEvent::Focused(true));
                with_app(move |app| {
                    app.force_repaint();
                    // O foco de verdade chega depois do pedido: só agora a caixa pode recebê-lo.
                    if got_focus && app.ui.get_mode() == BAR {
                        app.ui.invoke_focus_input();
                    }
                    if lost_focus {
                        let mode = app.ui.get_mode();
                        // Ouvindo um comando de voz: a barra fica aberta mesmo sem foco
                        // (chamada pelo "Nocto", ela nem chega a ganhar o foco).
                        if (mode == BAR || mode == CALENDAR) && !app.ui.get_listening() {
                            app.close(false);
                        }
                    }
                })
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

fn main() -> Result<(), slint::PlatformError> {
    if !win::single_instance(|| on_ui(|app| {
        if app.ui.get_mode() != BAR {
            app.toggle_bar();
        }
    })) {
        return Ok(());
    }

    let dir = data_dir();
    let config_path = dir.join("config.json");
    if !config_path.exists() {
        let _ = Config::default().save(&config_path);
    }
    let config = Config::load(&config_path);

    let ui = Island::new()?;
    wire_callbacks(&ui);
    ui.show()?;
    start_when_ready(ui, config, dir);
    slint::run_event_loop_until_quit()
}

/// O winit só cria a janela nativa depois que o laço de eventos começa.
fn start_when_ready(ui: Island, config: Config, dir: PathBuf) {
    slint::Timer::single_shot(Duration::from_millis(10), move || match win::hwnd_of(ui.window()) {
        Some(hwnd) => start(ui, hwnd, config, dir),
        None => start_when_ready(ui, config, dir),
    });
}

fn start(ui: Island, hwnd: isize, config: Config, dir: PathBuf) {
    win::setup(hwnd);
    win::place_fixed(hwnd, WINDOW_W, WINDOW_H, ui.window().scale_factor());
    let glass = if std::env::var_os("NOCTO_NOGLASS").is_some() { None } else { glass::Glass::new(hwnd).ok() };
    ui.set_solid(glass.is_none());
    if let Some(g) = &glass {
        win::place_fixed(g.hwnd, WINDOW_W, WINDOW_H, ui.window().scale_factor());
        // As duas janelas ficam sempre "mostradas"; para sumir, só a área visível muda.
        win::show(g.hwnd, true);
    }

    let (y, m, ..) = win::local_time();
    let hotkey_text = config.hotkey.clone();
    let app = App {
        ui: ui.clone_strong(),
        hwnd,
        allow: load_allowlist(&dir),
        cal: Calendar::load(&dir.join("calendar.json")),
        view: (y, m),
        selected: iso_today(),
        prev_foreground: 0,
        dropped: None,
        fullscreen_app: false,
        covered: false,
        peek: false,
        glass,
        last_shape: std::cell::Cell::new((0.0, 0.0)),
        region_spec: std::cell::Cell::new((0.0, 0.0, 0.0, 0.0)),
        hidden: std::cell::Cell::new(false),
        track: None,
        page: 0,
        progress_timer: slint::Timer::default(),
        karaoke: false,
        karaoke_timer: slint::Timer::default(),
        lyrics: None,
        lyrics_key: String::new(),
        sync_tick: 0,
        k_pos: 0.0,
        k_idx: None,
        k_clock: None,
        synced_at: None,
        pill_note: String::new(),
        volume: volume::Volume::new(),
        notify_serial: 0,
        clips: load_pinned_clips(&dir),
        clip_open: None,
        clip_serial: 0,
        timer_end: None,
        timer_total: 0,
        timer_tick: slint::Timer::default(),
        wake: None,
        wake_paused: Default::default(),
        wa_msgs: Vec::new(),
        last_msg: None,
        dev: DevState::default(),
        flash_serial: 0,
        preview_path: Default::default(),
        mail_msgs: Vec::new(),
        mail_source: Source::Email { web: true },
        notify_source: Source::WhatsApp,
        icons: RefCell::new(std::collections::HashMap::new()),
        config,
        dir,
    };
    app.apply_theme();
    APP.with(|cell| *cell.borrow_mut() = Some(app));

    // Atalho global.
    let notice = match keys::parse_hotkey(&hotkey_text) {
        Ok(hk) => match win::spawn_hotkey(hk, || on_ui(|app| app.toggle_bar())) {
            Ok(()) => format!("Nocto · {}", hotkey_text.replace('+', " + ")),
            Err(_) => format!("Atalho {hotkey_text} ocupado: troque em config.json"),
        },
        Err(e) => e,
    };
    ui.set_notice(notice.clone().into());
    // Alt + Shift + V: direto na área de transferência.
    if let Ok(hk) = keys::parse_hotkey("alt+shift+v") {
        let _ = win::spawn_hotkey(hk, || {
            on_ui(|app| {
                app.close_clip();
                app.open_panel(3);
            })
        });
    }
    // O que for copiado entra no histórico.
    clipboard::watch(|data| on_ui(move |app| app.add_clip(data)));
    with_app(move |app| {
        if notice.starts_with("Atalho") || !notice.starts_with("Nocto") {
            app.pill_note = notice;
        }
        app.refresh_clock();
        app.update_compact();
        app.set_mode(PILL);
        // Já começa escondida se um jogo em tela cheia estiver na frente.
        app.foreground_changed(win::foreground());
        // Desenvolvimento: NOCTO_SETVOL=0.0..1.0 define o volume do Spotify ao abrir.
        if let Some(v) = std::env::var("NOCTO_SETVOL").ok().and_then(|v| v.parse::<f32>().ok()) {
            app.volume.set(v);
        }
        // Desenvolvimento: NOCTO_WA simula uma mensagem do WhatsApp 2 s depois de abrir
        // (NOCTO_WA=mail simula um e-mail).
        if let Some(kind) = std::env::var_os("NOCTO_WA") {
            let mail = kind == "mail";
            slint::Timer::single_shot(Duration::from_secs(2), move || {
                with_app(move |app| {
                    if mail {
                        app.show_message(Source::Email { web: true }, vec!["Hacko".into(), "Sua inscrição foi confirmada".into()])
                    } else {
                        app.show_message(Source::WhatsApp, vec!["Maria".into(), "Bora jogar hoje à noite?".into()])
                    }
                })
            });
        }
        // Desenvolvimento: NOCTO_VOICE começa a ouvir logo ao abrir (com NOCTO_MODE=1).
        if std::env::var_os("NOCTO_VOICE").is_some() {
            app.ui.invoke_voice();
        }
        // Desenvolvimento: NOCTO_SETUP_SPOTIFY mostra o passo a passo nas Configurações.
        if std::env::var_os("NOCTO_SETUP_SPOTIFY").is_some() {
            app.ui.set_settings_note("1) Create app  2) Redirect URI: http://127.0.0.1:8898/callback  3) marque Web API  4) copie o Client ID e digite na barra: spotify id <código>".into());
        }
        // Desenvolvimento: NOCTO_CLIP_OPEN=i abre o preview do item i depois de 4 s.
        if let Some(i) = std::env::var("NOCTO_CLIP_OPEN").ok().and_then(|s| s.parse::<usize>().ok()) {
            slint::Timer::single_shot(Duration::from_secs(8), move || with_app(move |app| app.open_clip(i)));
        }
        // Desenvolvimento: NOCTO_KARAOKE liga o karaokê 3 s depois de abrir.
        if std::env::var_os("NOCTO_KARAOKE").is_some() {
            slint::Timer::single_shot(Duration::from_secs(3), || {
                with_app(|app| {
                    if app.track.is_none() {
                        app.track = Some(media::Track {
                            title: "Ghost Town".into(),
                            artist: "Kanye West".into(),
                            playing: true,
                            spotify: true,
                            position_ms: 18_000,
                            duration_ms: 271_000,
                            read_at: std::time::Instant::now(),
                            cover: None,
                        });
                        app.ui.set_song_title("Ghost Town".into());
                        app.ui.set_artist("Kanye West".into());
                        app.ui.set_has_track(true);
                    }
                    app.toggle_karaoke()
                })
            });
        }
        // Desenvolvimento: NOCTO_TIMER=segundos começa um timer ao abrir.
        if let Some(secs) = std::env::var("NOCTO_TIMER").ok().and_then(|s| s.parse().ok()) {
            app.start_timer(secs);
        }
        // Desenvolvimento: NOCTO_PLAYING simula música tocando na pílula.
        if std::env::var_os("NOCTO_PLAYING").is_some() {
            app.ui.set_track("Dark Fantasy — Kanye West".into());
            app.ui.set_has_track(true);
            app.ui.set_playing(true);
            app.ui.set_spotify(true);
            app.update_compact();
        }
        // Desenvolvimento: NOCTO_CYCLE alterna sozinho entre os formatos (para reproduzir bugs).
        if std::env::var_os("NOCTO_CYCLE").is_some() {
            let timer = Box::leak(Box::new(slint::Timer::default()));
            let step = std::cell::Cell::new(0);
            timer.start(slint::TimerMode::Repeated, Duration::from_millis(1500), move || {
                let s = step.get();
                step.set(s + 1);
                with_app(move |app| {
                    // Simula tocar/pausar: a pílula troca de música para relógio e muda de tamanho.
                    app.ui.set_has_track(true);
                    app.ui.set_playing(s % 2 == 0);
                    app.update_compact();
                })
            });
        }
        // Desenvolvimento: NOCTO_MODE=1|2|3 abre direto num modo, sem roubar o foco (para prints).
        if let Some(mode) = std::env::var("NOCTO_MODE").ok().and_then(|m| m.parse::<i32>().ok()) {
            if mode == DROP {
                // NOCTO_FILE=caminho simula soltar esse arquivo (com miniatura).
                let file = std::env::var("NOCTO_FILE").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("foto-perfil.png"));
                let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                app.ui.set_drop_name(name.into());
                app.show_drop_preview(&file, "Original");
                app.dropped = Some(file);
                // NOCTO_CONVERT=png|nobg… já clica na opção (para testar o resultado).
                if let Ok(fmt) = std::env::var("NOCTO_CONVERT") {
                    app.ui.invoke_convert(fmt.into());
                }
            }
            if mode == BAR || mode == DROP {
                app.ui.set_status(std::env::var("NOCTO_STATUS").unwrap_or_default().into());
            }
            app.set_mode(mode);
            if let Some(page) = std::env::var("NOCTO_PAGE").ok().and_then(|p| p.parse::<i32>().ok()) {
                app.go_page(page);
            }
        }
    });

    // Música atual, por evento do Windows.
    media::watch(|track| {
        if std::env::var_os("NOCTO_PLAYING").is_some() {
            return; // desenvolvimento: mantém a música simulada
        }
        on_ui(move |app| {
            let had = app.ui.get_has_track();
            let old_title = app.track.as_ref().map(|t| (t.title.clone(), t.artist.clone()));
            match &track {
                Some(t) => {
                    let label = if t.artist.is_empty() { t.title.clone() } else { format!("{} — {}", t.title, t.artist) };
                    app.ui.set_track(label.into());
                    app.ui.set_song_title(t.title.clone().into());
                    app.ui.set_artist(t.artist.clone().into());
                    app.ui.set_playing(t.playing);
                    app.ui.set_spotify(t.spotify);
                    if let Some(cover) = &t.cover {
                        let (w, h, px) = &**cover;
                        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(px, *w, *h);
                        app.ui.set_cover(slint::Image::from_rgba8(buf));
                        app.ui.set_has_cover(true);
                    } else if old_title != Some((t.title.clone(), t.artist.clone())) {
                        app.ui.set_has_cover(false);
                    }
                }
                None => {
                    app.ui.set_playing(false);
                    app.ui.set_has_cover(false);
                }
            }
            app.ui.set_has_track(track.is_some());
            // A posição do Windows atrasa: se a da API do Spotify é recente e a música é a
            // mesma, mantém a da API (senão a letra pulava para trás).
            let fresh = app.synced_at.is_some_and(|s| s.elapsed() < Duration::from_secs(5));
            let prev = app.track.as_ref().map(|t| (t.title.clone(), t.playing, t.position_ms, t.read_at));
            app.track = track.map(|mut t| {
                t.cover = None;
                if let Some((title, playing, pos, at)) = &prev {
                    if fresh && *title == t.title && *playing == t.playing {
                        t.position_ms = *pos;
                        t.read_at = *at;
                    }
                }
                t
            });
            app.update_compact();
            app.update_progress_timer();
            if app.karaoke {
                app.load_lyrics();
            }
            // Música mudou com a página aberta e o knob sem volume: tenta achar de novo
            // (o Spotify só cria a sessão de áudio quando começa a tocar).
            if app.ui.get_mode() == CALENDAR && !app.ui.get_has_volume() {
                app.refresh_volume();
            }
            if had != app.ui.get_has_track() && app.ui.get_mode() == PILL {
                app.layout();
            }
        })
    });

    watch_devices();
    // Mensagens do WhatsApp, pelas notificações do Windows.
    notify::watch(|source, texts| on_ui(move |app| app.show_message(source, texts)));
    with_app(|app| {
        app.apply_wake();
        win::set_autostart(app.config.autostart);
    });

    // Troca de tema: uma thread que dorme até a próxima virada (05h, 16h, 18h).
    std::thread::spawn(|| loop {
        let (_, _, _, h, m, s) = win::local_time();
        std::thread::sleep(theme::next_switch_in(h, m, s) + Duration::from_secs(1));
        on_ui(|app| app.apply_theme());
    });

    // Esconde a pílula quando um app em tela cheia (jogo, vídeo) vem para a frente.
    win::watch_foreground(|fg| with_app(move |app| app.foreground_changed(fg)));
    let cover_timer = slint::Timer::default();
    cover_timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), || with_app(|app| app.check_cover()));
    std::mem::forget(cover_timer);
}
