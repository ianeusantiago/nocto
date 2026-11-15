//! Controles de mídia do Windows (SMTC): faixa atual, capa, tempo, play/pause e troca
//! de faixa. Funciona com Spotify e qualquer player que se registre no Windows. Sem
//! login e sem polling: o Windows avisa por evento quando algo muda.

use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

/// Capa decodificada: largura, altura e pixels RGBA.
pub type Cover = (u32, u32, Vec<u8>);

#[derive(Debug, Clone)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub spotify: bool,
    /// Posição no momento `read_at` (ms).
    pub position_ms: u64,
    pub duration_ms: u64,
    pub read_at: Instant,
    /// Só vem quando a música muda (para não decodificar a capa à toa).
    pub cover: Option<Arc<Cover>>,
}

fn manager() -> Option<Manager> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    Manager::RequestAsync().ok()?.join().ok()
}

/// Agora, no mesmo relógio do Windows usado em LastUpdatedTime (100 ns desde 1601).
fn now_ticks() -> i64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

fn read_cover(session: &Session) -> Option<Cover> {
    let props = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    nocto_core::imgconv::decode_rgba(&bytes, 256)
}

fn read(mgr: &Manager, last_key: &Mutex<String>) -> Option<Track> {
    let session = mgr.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = props.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    let artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
    let playing = session.GetPlaybackInfo().ok().and_then(|p| p.PlaybackStatus().ok()) == Some(Status::Playing);
    let spotify = session
        .SourceAppUserModelId()
        .map(|id| id.to_string().to_lowercase().contains("spotify"))
        .unwrap_or(false);

    let (mut position_ms, mut duration_ms) = (0, 0);
    if let Ok(tl) = session.GetTimelineProperties() {
        let pos = tl.Position().map(|t| t.Duration).unwrap_or(0);
        let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
        let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
        let mut pos = pos - start;
        // A posição informada pode ser antiga: soma o tempo desde a última atualização.
        if playing {
            if let Ok(updated) = tl.LastUpdatedTime() {
                let elapsed = now_ticks() - updated.UniversalTime;
                if (0..10 * 60 * 10_000_000).contains(&elapsed) {
                    pos += elapsed;
                }
            }
        }
        position_ms = (pos.max(0) / 10_000) as u64;
        duration_ms = ((end - start).max(0) / 10_000) as u64;
    }

    let key = format!("{title}\u{1}{artist}");
    let changed = {
        let mut last = last_key.lock().unwrap();
        let changed = *last != key;
        *last = key;
        changed
    };
    let cover = if changed { read_cover(&session).map(Arc::new) } else { None };

    Some(Track { title, artist, playing, spotify, position_ms, duration_ms, read_at: Instant::now(), cover })
}

/// Chama `on_change` com a faixa atual agora e a cada mudança.
pub fn watch(on_change: impl Fn(Option<Track>) + Send + Sync + 'static) {
    std::thread::spawn(move || {
        let Some(mgr) = manager() else { return };
        let on_change = Arc::new(on_change);
        let last_key = Arc::new(Mutex::new(String::new()));
        let push = {
            let mgr = mgr.clone();
            let f = on_change.clone();
            Arc::new(move || f(read(&mgr, &last_key)))
        };
        let subscribe = {
            let push = push.clone();
            Arc::new(move |mgr: &Manager| {
                if let Ok(s) = mgr.GetCurrentSession() {
                    let p = push.clone();
                    let _ = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                        p();
                        Ok(())
                    }));
                    let p = push.clone();
                    let _ = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                        p();
                        Ok(())
                    }));
                    let p = push.clone();
                    let _ = s.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                        p();
                        Ok(())
                    }));
                }
            })
        };
        subscribe(&mgr);
        {
            let mgr2 = mgr.clone();
            let push = push.clone();
            let subscribe = subscribe.clone();
            let _ = mgr.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                subscribe(&mgr2);
                push();
                Ok(())
            }));
        }
        push();
        // Mantém o gerenciador e as inscrições vivos; a thread fica parada sem custo.
        loop {
            std::thread::park();
        }
    });
}

fn with_session(action: fn(&Session)) {
    std::thread::spawn(move || {
        if let Some(s) = manager().and_then(|m| m.GetCurrentSession().ok()) {
            action(&s);
        }
    });
}

pub fn play_pause() {
    with_session(|s| {
        let _ = s.TryTogglePlayPauseAsync().and_then(|op| op.join());
    });
}

pub fn next() {
    with_session(|s| {
        let _ = s.TrySkipNextAsync().and_then(|op| op.join());
    });
}

pub fn previous() {
    with_session(|s| {
        let _ = s.TrySkipPreviousAsync().and_then(|op| op.join());
    });
}
