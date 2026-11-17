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

/// O ouvido percebe volume em escala logarítmica: com o valor direto do mixer, 13% já
/// soava alto e de 70% a 100% quase não mudava. O knob usa uma curva cúbica (≈ decibéis):
/// 50% do knob ≈ -18 dB, 13% quase mudo, e cada trecho do knob muda o som por igual.
fn to_gain(knob: f32) -> f32 {
    knob.clamp(0.0, 1.0).powi(3)
}

fn from_gain(gain: f32) -> f32 {
    gain.clamp(0.0, 1.0).cbrt()
}

/// Sessões de áudio do Spotify (ele pode ter mais de um processo tocando).
fn spotify_sessions() -> Vec<ISimpleAudioVolume> {
    let mut out = Vec::new();
    unsafe {
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return out;
        };
        // Todas as saídas ativas, não só a padrão: o Spotify pode estar tocando no fone
        // ou numa saída escolhida no próprio app.
        let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for d in 0..devices.GetCount().unwrap_or(0) {
            let Ok(device) = devices.Item(d) else { continue };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
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
                    let _ = s.SetMasterVolume(to_gain(v), std::ptr::null());
                }
            }
        }
        let current = sessions.first().and_then(|s| unsafe { s.GetMasterVolume().ok() }).map(from_gain);
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
