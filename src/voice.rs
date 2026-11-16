//! Comando por voz com o reconhecimento de fala do próprio Windows (grátis, sem
//! instalar nada). Só roda enquanto você está falando; parado, não custa nada.

use windows::Globalization::Language;
use windows::Media::SpeechRecognition::{SpeechRecognitionResultStatus, SpeechRecognizer};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::HSTRING;

/// Ouve uma frase (para sozinho quando você para de falar) e devolve o texto.
pub fn listen(done: impl FnOnce(Result<String, String>) + Send + 'static) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        done(recognize());
    });
}

fn recognize() -> Result<String, String> {
    let rec = Language::CreateLanguage(&HSTRING::from("pt-BR"))
        .and_then(|lang| SpeechRecognizer::Create(&lang))
        .or_else(|_| SpeechRecognizer::new())
        .map_err(|e| explain(e.code().0))?;
    let compiled = rec.CompileConstraintsAsync().and_then(|op| op.join()).map_err(|e| explain(e.code().0))?;
    if compiled.Status().ok() != Some(SpeechRecognitionResultStatus::Success) {
        return Err(explain_status(compiled.Status().ok()));
    }
    let result = rec.RecognizeAsync().and_then(|op| op.join()).map_err(|e| explain(e.code().0))?;
    let status = result.Status().ok();
    if status != Some(SpeechRecognitionResultStatus::Success) {
        return Err(explain_status(status));
    }
    let text = result.Text().map(|t| t.to_string()).unwrap_or_default();
    let text = text.trim().trim_end_matches(['.', '!', '?']).to_string();
    if text.is_empty() { Err("Não ouvi nada. Tente de novo.".into()) } else { Ok(text) }
}

fn explain(code: i32) -> String {
    match code as u32 {
        // SPERR_SPEECH_PRIVACY_POLICY_NOT_ACCEPTED
        0x80045509 => "Ative “Reconhecimento de fala online” em Configurações › Privacidade › Fala.".into(),
        0x80070005 => "Sem acesso ao microfone: libere em Configurações › Privacidade › Microfone.".into(),
        _ => format!("Voz indisponível (0x{:08x}).", code as u32),
    }
}

fn explain_status(status: Option<SpeechRecognitionResultStatus>) -> String {
    match status {
        Some(SpeechRecognitionResultStatus::MicrophoneUnavailable) => "Nenhum microfone encontrado.".into(),
        Some(SpeechRecognitionResultStatus::NetworkFailure) => "Sem internet para reconhecer a fala.".into(),
        Some(SpeechRecognitionResultStatus::TimeoutExceeded) => "Não ouvi nada. Tente de novo.".into(),
        Some(SpeechRecognitionResultStatus::UserCanceled) => "Cancelado.".into(),
        _ => "Não entendi. Tente de novo.".into(),
    }
}
