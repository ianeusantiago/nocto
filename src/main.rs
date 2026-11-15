#![windows_subsystem = "windows"]

mod glass;
mod media;
mod volume;
mod win;

use nocto_core::calendar::{self, Calendar};
use nocto_core::commands::{self, Command};
use nocto_core::config::{self, Config};
use nocto_core::imgconv::{self, Format};
use nocto_core::launcher::{self, Allowlist};
use nocto_core::{format, keys, spotify, theme};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{Color, ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

slint::include_modules!();

const PILL: i32 = 0;
const BAR: i32 = 1;
const CALENDAR: i32 = 2;
const DROP: i32 = 3;

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
    glass: Option<glass::Glass>,
    last_shape: std::cell::Cell<(f32, f32)>,
    region_spec: std::cell::Cell<(f32, f32, f32, f32)>,
    hidden: std::cell::Cell<bool>,
    /// Última faixa informada pelo Windows (sem a capa, que já foi para a tela).
    track: Option<media::Track>,
    /// Página do painel: 0 música, 1 calendário.
    page: i32,
    progress_timer: slint::Timer,
    /// Aviso que substitui a data na pílula (ex.: atalho ocupado).
    pill_note: String,
    volume: volume::Volume,
}

/// Janela fixa (lógica) onde a ilha se transforma; cabe o maior modo (calendário).
const WINDOW_W: f32 = 720.0;
const WINDOW_H: f32 = 360.0;

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

fn color(c: [u8; 4]) -> Color {
    Color::from_argb_u8(c[3], c[0], c[1], c[2])
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
    fn visible(&self, visible: bool) {
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
            CALENDAR if self.page == 0 => (680.0, 214.0, 40.0),
            CALENDAR => (680.0, 340.0, 40.0),
            DROP => (440.0, 128.0, 36.0),
            _ if ui.get_show_music() => (360.0, 38.0, 19.0),
            _ => (230.0, 34.0, 17.0),
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
        self.visible(!(self.ui.get_mode() == PILL && self.fullscreen_app));
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
        self.ui.set_page(page);
        if page == 1 {
            self.refresh_calendar();
        }
        self.layout();
        self.update_progress_timer();
    }

    /// Abre o painel: na música se houver uma; senão, no calendário.
    fn open_panel(&mut self, page: i32) {
        self.page = page;
        self.ui.set_page(page);
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
        let run = self.ui.get_mode() == CALENDAR && self.page == 0 && self.ui.get_playing();
        if run && !self.progress_timer.running() {
            self.progress_timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
                with_app(|app| app.refresh_progress())
            });
        } else if !run {
            self.progress_timer.stop();
        }
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
            let _ = glass.set_tint([a, r, g, b], 3000);
        }
        self.ui.set_accent(color(p.accent));
        self.ui.set_ink(color(p.text));
        self.ui.set_theme_label(self.config.theme.label().into());
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

    fn spotify_config(&self) -> spotify::Config {
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
                            " (edite allowlist.json para adicionar apps)"
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

    fn convert(&mut self, fmt: &str) {
        let Some(src) = self.dropped.clone() else { return };
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
            if self.ui.get_mode() == PILL {
                self.visible(!full);
            }
        }
        self.apply_theme();
    }
}

fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
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
            let page = if app.ui.get_has_track() { 0 } else { 1 };
            app.open_panel(page);
        })
    });
    ui.on_prev_track(media::previous);
    ui.on_go_page(|page| with_app(move |app| app.go_page(page)));
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
    ui.on_cycle_compact(|| {
        with_app(|app| {
            app.config.compact = app.config.compact.next();
            let _ = app.config.save(&app.dir.join("config.json"));
            app.update_compact();
        })
    });
    ui.on_play_pause(media::play_pause);
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
            WindowEvent::HoveredFile(_) => with_app(|app| {
                if app.ui.get_mode() != DROP {
                    app.dropped = None;
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
                    app.dropped = Some(path);
                    app.ui.set_status("Converter para:".into());
                    app.ui.set_status_error(false);
                    app.set_mode(DROP);
                })
            }
            WindowEvent::HoveredFileCancelled => with_app(|app| {
                if app.dropped.is_none() {
                    app.close(false);
                }
            }),
            WindowEvent::Focused(_) | WindowEvent::Resized(_) | WindowEvent::Occluded(false) | WindowEvent::ScaleFactorChanged { .. } => {
                let lost_focus = matches!(event, WindowEvent::Focused(false));
                with_app(move |app| {
                    app.force_repaint();
                    if lost_focus {
                        let mode = app.ui.get_mode();
                        if mode == BAR || mode == CALENDAR {
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
        glass,
        last_shape: std::cell::Cell::new((0.0, 0.0)),
        region_spec: std::cell::Cell::new((0.0, 0.0, 0.0, 0.0)),
        hidden: std::cell::Cell::new(false),
        track: None,
        page: 0,
        progress_timer: slint::Timer::default(),
        pill_note: String::new(),
        volume: volume::Volume::new(),
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
                app.ui.set_drop_name("foto-perfil.png".into());
                app.dropped = Some(PathBuf::from("foto-perfil.png"));
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
            app.track = track.map(|mut t| {
                t.cover = None;
                t
            });
            app.update_compact();
            app.update_progress_timer();
            if had != app.ui.get_has_track() && app.ui.get_mode() == PILL {
                app.layout();
            }
        })
    });

    // Troca de tema: uma thread que dorme até a próxima virada (05h, 16h, 18h).
    std::thread::spawn(|| loop {
        let (_, _, _, h, m, s) = win::local_time();
        std::thread::sleep(theme::next_switch_in(h, m, s) + Duration::from_secs(1));
        on_ui(|app| app.apply_theme());
    });

    // Esconde a pílula quando um app em tela cheia (jogo, vídeo) vem para a frente.
    win::watch_foreground(|fg| with_app(move |app| app.foreground_changed(fg)));
}
