//! Volume do Spotify pelo mixer do Windows (o mesmo do "Mixer de volume"): local,
//! sem login e sem internet. Muda só o volume do app, não o do sistema.
//!
//! Tudo roda numa thread própria: pedidos de mudança chegam por um canal e só o
//! mais recente é aplicado (girar o knob gera muitos eventos seguidos).

use std::sync::mpsc::{Receiver, Sender, channel};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::core::Interface;

enum Request {
    Get(Box<dyn FnOnce(Option<f32>) + Send>),
    Set(f32),
}

pub struct Volume {
    tx: Sender<Request>,
}

/// Sessões de áudio do Spotify (ele pode ter mais de um processo tocando).
fn spotify_sessions() -> Vec<ISimpleAudioVolume> {
    let mut out = Vec::new();
    unsafe {
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return out;
        };
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return out };
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
        let Ok(sessions) = manager.GetSessionEnumerator() else { return out };
        let count = sessions.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(control) = sessions.GetSession(i) else { continue };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
            let Ok(pid) = control2.GetProcessId() else { continue };
            if crate::win::process_name(pid).is_some_and(|n| n == "spotify.exe") {
                if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                    out.push(volume);
                }
            }
        }
    }
    out
}

fn worker(rx: Receiver<Request>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    while let Ok(first) = rx.recv() {
        // Junta os pedidos acumulados: vale o último "Set"; os "Get" são todos atendidos.
        let mut gets = Vec::new();
        let mut set = None;
        for req in std::iter::once(first).chain(rx.try_iter()) {
            match req {
                Request::Get(reply) => gets.push(reply),
                Request::Set(v) => set = Some(v),
            }
        }
        let sessions = spotify_sessions();
        if let Some(v) = set {
            for s in &sessions {
                unsafe {
                    let _ = s.SetMasterVolume(v.clamp(0.0, 1.0), std::ptr::null());
                }
            }
        }
        let current = sessions.first().and_then(|s| unsafe { s.GetMasterVolume().ok() });
        for reply in gets {
            reply(current);
        }
    }
}

impl Volume {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || worker(rx));
        Self { tx }
    }

    /// Lê o volume atual (0–1); `None` se o Spotify não estiver tocando som.
    pub fn get(&self, reply: impl FnOnce(Option<f32>) + Send + 'static) {
        let _ = self.tx.send(Request::Get(Box::new(reply)));
    }

    pub fn set(&self, value: f32) {
        let _ = self.tx.send(Request::Set(value));
    }
}
