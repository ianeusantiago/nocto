//! Baixa (uma vez) o motor de voz offline Vosk e o modelo em português.
//! Tudo grátis e local: depois de baixado, nada sai do seu PC.

use std::io::Read;
use std::path::{Path, PathBuf};

const ENGINE_URL: &str = "https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip";
const MODEL_URL: &str = "https://alphacephei.com/vosk/models/vosk-model-small-pt-0.3.zip";
/// DLLs que o motor precisa (o resto do pacote não serve para nós).
pub const DLLS: [&str; 4] = ["libvosk.dll", "libstdc++-6.dll", "libwinpthread-1.dll", "libgcc_s_seh-1.dll"];

/// Pasta `<dados>/vosk`, com as DLLs e `model/`. Baixa o que faltar.
pub fn ensure(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("vosk");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !DLLS.iter().all(|d| dir.join(d).exists()) {
        let zip = download(ENGINE_URL)?;
        extract(&zip, &dir, |name| DLLS.iter().any(|d| name.ends_with(&format!("/{d}"))).then(|| name.rsplit('/').next().unwrap().into()))?;
    }
    if !dir.join("model").join("final.mdl").exists() {
        let zip = download(MODEL_URL)?;
        let tmp = dir.join("model.part");
        let _ = std::fs::remove_dir_all(&tmp);
        // "vosk-model-small-pt-0.3/x/y" → "x/y"
        extract(&zip, &tmp, |name| name.split_once('/').map(|(_, rest)| rest.into()).filter(|r: &String| !r.is_empty()))?;
        let _ = std::fs::remove_dir_all(dir.join("model"));
        std::fs::rename(&tmp, dir.join("model")).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    ureq::get(url)
        .call()
        .map_err(|e| format!("não consegui baixar a voz offline: {e}"))?
        .into_body()
        .into_reader()
        .read_to_end(&mut out)
        .map_err(|e| format!("download interrompido: {e}"))?;
    Ok(out)
}

/// Extrai do zip os arquivos para os quais `rename` devolve um caminho relativo.
fn extract(bytes: &[u8], to: &Path, rename: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("zip inválido: {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let Ok(name) = f.name() else { continue };
        let Some(rel) = rename(&name) else { continue };
        if rel.contains("..") {
            continue;
        }
        let path = to.join(&rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}
