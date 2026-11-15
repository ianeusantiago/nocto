//! Controles de mídia do Windows (SMTC): faixa atual, play/pause e próxima.
//! Funciona com Spotify e qualquer player que se registre no Windows. Sem login, sem polling:
//! o Windows avisa por evento quando a música ou o estado mudam.

use std::sync::Arc;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

#[derive(Debug, Clone)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

fn manager() -> Option<Manager> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    Manager::RequestAsync().ok()?.join().ok()
}

fn read(mgr: &Manager) -> Option<Track> {
    let session = mgr.GetCurrentSession().ok()?;
    let props = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = props.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    let artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
    let playing = session.GetPlaybackInfo().ok().and_then(|p| p.PlaybackStatus().ok()) == Some(Status::Playing);
    Some(Track { title, artist, playing })
}

/// Chama `on_change` com a faixa atual agora e a cada mudança.
pub fn watch(on_change: impl Fn(Option<Track>) + Send + Sync + 'static) {
    std::thread::spawn(move || {
        let Some(mgr) = manager() else { return };
        let on_change = Arc::new(on_change);
        let push = {
            let mgr = mgr.clone();
            let f = on_change.clone();
            Arc::new(move || f(read(&mgr)))
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

fn with_session(action: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSession)) {
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
