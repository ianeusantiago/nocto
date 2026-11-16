//! "Nocto": dizer o nome abre a barra e já começa a ouvir o comando.
//!
//! Para gastar pouco, o microfone fica ligado mas o reconhecimento só roda quando há
//! som acima do ruído do ambiente, e só em trechos curtos (o nome dura menos de 1 s).
//! Em silêncio, o custo é só medir o volume. Em jogo de tela cheia, pausa por completo.

use crate::voice::{Engine, open_mic, resample};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Como o modelo em português escreve "Nocto".
const GRAMMAR: &str = r#"["noto","nó","noite","nota","nossa","[unk]"]"#;
/// Palavras de verdade que soam parecido: se a leitura livre for uma delas, não era o nome.
const LOOKALIKES: [&str; 5] = ["nota", "noite", "nossa", "note", "notas"];

pub struct Wake {
    stop: Arc<AtomicBool>,
}

impl Drop for Wake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Começa a escutar. `paused` (jogo, barra aberta…) faz ignorar o áudio. Soltar o
/// `Wake` desliga o microfone e libera o modelo da memória.
pub fn start(data_dir: std::path::PathBuf, paused: Arc<AtomicBool>, on_wake: impl Fn() + Send + 'static) -> Wake {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        let Ok(dir) = nocto_core::voskdl::ensure(&data_dir) else { return };
        let Ok(engine) = Engine::load(&dir) else { return };
        let Ok((stream, rx, rate, channels)) = open_mic() else { return };
        let block_secs = |n: usize| n as f32 / rate as f32;
        let mut noise = 0.01f32;
        let mut seg: Vec<f32> = Vec::new();
        let mut pre: Vec<f32> = Vec::new(); // um pedacinho antes da fala, para não cortar o "N"
        let mut quiet = 0.0f32;
        while !stop2.load(Ordering::Relaxed) {
            let Ok(block) = rx.recv_timeout(Duration::from_millis(300)) else { continue };
            if paused.load(Ordering::Relaxed) {
                seg.clear();
                continue;
            }
            let mono: Vec<f32> = block.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
            let rms = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len().max(1) as f32).sqrt();
            let loud = rms > (noise * 3.0).max(0.015);
            if seg.is_empty() {
                if loud {
                    seg.extend_from_slice(&pre);
                    seg.extend_from_slice(&mono);
                    quiet = 0.0;
                } else {
                    noise = noise * 0.95 + rms * 0.05;
                    pre = mono;
                }
                continue;
            }
            seg.extend_from_slice(&mono);
            quiet = if loud { 0.0 } else { quiet + block_secs(mono.len()) };
            let len = block_secs(seg.len());
            if len > 2.5 {
                // Fala comprida (conversa, música): não é o nome. Descarta sem reconhecer.
                seg.clear();
                continue;
            }
            if quiet < 0.35 {
                continue;
            }
            let audio = resample(&std::mem::take(&mut seg), rate);
            if len < 0.3 {
                continue;
            }
            let picked = engine.transcribe(&audio, Some(GRAMMAR));
            if !picked.split(' ').any(|w| w == "noto") {
                continue;
            }
            let free = engine.transcribe(&audio, None);
            if free.split(' ').any(|w| LOOKALIKES.contains(&w)) {
                continue;
            }
            on_wake();
        }
        drop(stream);
    });
    Wake { stop }
}
