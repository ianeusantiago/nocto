//! Comando por voz 100% offline: grava o microfone até você parar de falar e
//! transcreve com o Vosk (modelo pequeno em português), tudo no seu PC.
//! O motor só é carregado quando você clica no microfone; parado, não custa nada.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::ffi::{CStr, CString, c_char, c_float, c_int, c_short, c_void};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Ouve uma frase e devolve o texto. `status` recebe avisos de progresso (download, ouvindo…).
pub fn listen(
    data_dir: std::path::PathBuf,
    apps: Vec<String>,
    status: impl Fn(&'static str) + Send + 'static,
    done: impl FnOnce(Result<String, String>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let result = (|| {
            let first = !data_dir.join("vosk").join("model").join("final.mdl").exists();
            if first {
                status("Baixando a voz offline (50 MB, só na primeira vez)…");
            }
            let dir = nocto_core::voskdl::ensure(&data_dir)?;
            // Carrega o motor enquanto grava: quando você terminar de falar, já está pronto.
            let (tx, rx) = mpsc::channel();
            let dir2 = dir.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Engine::load(&dir2));
            });
            status("Ouvindo… fale o comando");
            let audio = record()?;
            status("Entendendo…");
            let engine = rx.recv().map_err(|_| "falha ao carregar a voz".to_string())??;
            let picked = engine.transcribe(&audio, Some(&nocto_core::voicefix::grammar(&apps)));
            let free = engine.transcribe(&audio, None);
            let text = if picked.replace("[unk]", "").trim().is_empty() && free.is_empty() {
                String::new()
            } else {
                nocto_core::voicefix::command(&picked, &free, &apps)
            };
            if text.is_empty() { Err("Não entendi. Tente de novo, mais perto do microfone.".into()) } else { Ok(text) }
        })();
        done(result);
    });
}

type ModelNew = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type RecNew = unsafe extern "C" fn(*mut c_void, c_float) -> *mut c_void;
type RecNewGrm = unsafe extern "C" fn(*mut c_void, c_float, *const c_char) -> *mut c_void;
type Accept = unsafe extern "C" fn(*mut c_void, *const c_short, c_int) -> c_int;
type Final = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type Free = unsafe extern "C" fn(*mut c_void);
type LogLevel = unsafe extern "C" fn(c_int);

pub struct Engine {
    lib: libloading::Library,
    model: *mut c_void,
}

unsafe impl Send for Engine {}

impl Engine {
    pub fn load(dir: &Path) -> Result<Engine, String> {
        use std::os::windows::ffi::OsStrExt;
        unsafe {
            // As DLLs de apoio (libstdc++ etc.) ficam na mesma pasta.
            let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
            let _ = windows::Win32::System::LibraryLoader::SetDllDirectoryW(windows::core::PCWSTR(wide.as_ptr()));
            let lib = libloading::Library::new(dir.join("libvosk.dll")).map_err(|e| format!("falha ao carregar a voz: {e}"))?;
            if let Ok(f) = lib.get::<LogLevel>(b"vosk_set_log_level") {
                f(-1);
            }
            let path = CString::new(dir.join("model").to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
            let model = lib.get::<ModelNew>(b"vosk_model_new").map_err(|e| e.to_string())?(path.as_ptr());
            if model.is_null() {
                return Err("modelo de voz corrompido; apague a pasta Nocto\\vosk".into());
            }
            Ok(Engine { lib, model })
        }
    }

    /// Áudio 16 kHz mono → texto. Com `grammar` (lista JSON de frases), só escolhe entre elas.
    pub fn transcribe(&self, audio: &[i16], grammar: Option<&str>) -> String {
        unsafe {
            let (Ok(new), Ok(new_grm), Ok(accept), Ok(fin), Ok(free)) = (
                self.lib.get::<RecNew>(b"vosk_recognizer_new"),
                self.lib.get::<RecNewGrm>(b"vosk_recognizer_new_grm"),
                self.lib.get::<Accept>(b"vosk_recognizer_accept_waveform_s"),
                self.lib.get::<Final>(b"vosk_recognizer_final_result"),
                self.lib.get::<Free>(b"vosk_recognizer_free"),
            ) else {
                return String::new();
            };
            let grm = grammar.and_then(|g| CString::new(g).ok());
            let rec = match &grm {
                Some(g) => new_grm(self.model, 16000.0, g.as_ptr()),
                None => new(self.model, 16000.0),
            };
            if rec.is_null() {
                return String::new();
            }
            for chunk in audio.chunks(8000) {
                accept(rec, chunk.as_ptr(), chunk.len() as c_int);
            }
            let json = CStr::from_ptr(fin(rec)).to_string_lossy().into_owned();
            free(rec);
            serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| v["text"].as_str().map(|s| s.trim().to_string()))
                .unwrap_or_default()
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            if let Ok(free) = self.lib.get::<Free>(b"vosk_model_free") {
                free(self.model);
            }
        }
    }
}

/// Grava do microfone padrão até 0,9 s de silêncio depois da fala (máx. 8 s;
/// desiste se ninguém falar em 5 s). Devolve 16 kHz mono.
fn record() -> Result<Vec<i16>, String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("Nenhum microfone encontrado.")?;
    let config = device.default_input_config().map_err(|e| format!("microfone: {e}"))?;
    let rate = config.sample_rate() as usize;
    let channels = config.channels() as usize;
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let err = |e| eprintln!("microfone: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(config.clone().into(), move |d: &[f32], _: &_| { let _ = tx.send(d.to_vec()); }, err, None)
        }
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.clone().into(),
            move |d: &[i16], _: &_| { let _ = tx.send(d.iter().map(|&s| s as f32 / 32768.0).collect()); },
            err,
            None,
        ),
        other => return Err(format!("formato de microfone não suportado: {other:?}")),
    }
    .map_err(|e| format!("microfone: {e}"))?;
    stream.play().map_err(|e| format!("microfone: {e}"))?;

    let start = Instant::now();
    let mut mono: Vec<f32> = Vec::new();
    let mut heard = false;
    let mut last_voice = Instant::now();
    let mut noise = 0.0f32; // nível do ambiente, medido no começo
    while start.elapsed() < Duration::from_secs(8) {
        let Ok(block) = rx.recv_timeout(Duration::from_millis(200)) else { continue };
        let frames: Vec<f32> = block.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
        let rms = (frames.iter().map(|s| s * s).sum::<f32>() / frames.len().max(1) as f32).sqrt();
        mono.extend(frames);
        if start.elapsed() < Duration::from_millis(300) {
            noise = noise.max(rms);
            continue;
        }
        if rms > (noise * 2.5).max(0.012) {
            heard = true;
            last_voice = Instant::now();
        } else if heard && last_voice.elapsed() > Duration::from_millis(900) {
            break;
        }
        if !heard && start.elapsed() > Duration::from_secs(5) {
            return Err("Não ouvi nada. Tente de novo.".into());
        }
    }
    drop(stream);
    Ok(resample(&mono, rate))
}

/// Reamostra para 16 kHz (média por janela: suficiente para voz).
pub fn resample(mono: &[f32], rate: usize) -> Vec<i16> {
    let step = rate as f64 / 16000.0;
    let n = (mono.len() as f64 / step) as usize;
    (0..n)
        .map(|i| {
            let a = (i as f64 * step) as usize;
            let b = (((i + 1) as f64 * step) as usize).clamp(a + 1, mono.len());
            let v = mono[a..b].iter().sum::<f32>() / (b - a) as f32;
            (v.clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}
