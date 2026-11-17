//! Área de transferência do Windows: avisa quando algo é copiado (sem ficar
//! consultando: o Windows manda WM_CLIPBOARDUPDATE) e copia de volta um item escolhido.
//!
//! Respeita os apps que pedem para não guardar (gerenciadores de senha marcam o que
//! copiam com "ExcludeClipboardContentFromMonitorProcessing").

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

/// O que foi copiado.
#[derive(Clone)]
pub enum Data {
    Text(String),
    Files(Vec<PathBuf>),
    /// Imagem RGBA (largura, altura, pixels).
    Image(u32, u32, std::sync::Arc<Vec<u8>>),
}

/// Número de sequência da área de transferência depois da nossa própria cópia:
/// essa mudança não vira item novo.
static OWN_SEQ: AtomicU32 = AtomicU32::new(0);

/// Começa a ouvir. `on_copy` roda numa thread própria a cada cópia aceita.
pub fn watch(on_copy: impl Fn(Data) + Send + 'static) {
    std::thread::spawn(move || unsafe {
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            None,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        ) else {
            return;
        };
        if AddClipboardFormatListener(hwnd).is_err() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_CLIPBOARDUPDATE || (msg.hwnd == hwnd && msg.message == WM_CLIPBOARDUPDATE) {
                if GetClipboardSequenceNumber() == OWN_SEQ.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(data) = read(hwnd) {
                    on_copy(data);
                }
            }
            DispatchMessageW(&msg);
        }
    });
}

unsafe fn open(hwnd: HWND) -> bool {
    // Outro app pode estar com a área de transferência aberta: tenta algumas vezes.
    for _ in 0..10 {
        if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    false
}

unsafe fn excluded() -> bool {
    unsafe {
        let skip = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        let ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
        if IsClipboardFormatAvailable(skip).is_ok() || IsClipboardFormatAvailable(ignore).is_ok() {
            return true;
        }
        // "CanIncludeInClipboardHistory" = 0: o app pediu para não entrar no histórico.
        let can = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if IsClipboardFormatAvailable(can).is_ok() {
            if let Ok(h) = GetClipboardData(can) {
                let p = GlobalLock(HGLOBAL(h.0)) as *const u32;
                let v = if p.is_null() { 1 } else { *p };
                let _ = GlobalUnlock(HGLOBAL(h.0));
                if v == 0 {
                    return true;
                }
            }
        }
        false
    }
}

unsafe fn read(hwnd: HWND) -> Option<Data> {
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let data = (|| {
            if excluded() {
                return None;
            }
            if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() {
                let h = GetClipboardData(CF_HDROP.0 as u32).ok()?;
                let drop = HDROP(h.0);
                let n = DragQueryFileW(drop, u32::MAX, None);
                let mut files = Vec::new();
                for i in 0..n.min(50) {
                    let len = DragQueryFileW(drop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    DragQueryFileW(drop, i, Some(&mut buf));
                    files.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
                }
                return (!files.is_empty()).then_some(Data::Files(files));
            }
            if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok() {
                let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if p.is_null() {
                    return None;
                }
                let size = GlobalSize(g) / 2;
                let slice = std::slice::from_raw_parts(p, size);
                let end = slice.iter().position(|&c| c == 0).unwrap_or(size);
                let text = String::from_utf16_lossy(&slice[..end]);
                let _ = GlobalUnlock(g);
                // Textos gigantes (um arquivo inteiro de log) não cabem no histórico.
                return (!text.trim().is_empty() && text.len() < 2_000_000).then_some(Data::Text(text));
            }
            if IsClipboardFormatAvailable(CF_DIB.0 as u32).is_ok() {
                let h = GetClipboardData(CF_DIB.0 as u32).ok()?;
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u8;
                if p.is_null() {
                    return None;
                }
                let bytes = std::slice::from_raw_parts(p, GlobalSize(g)).to_vec();
                let _ = GlobalUnlock(g);
                let (w, h, rgba) = nocto_core::imgconv::decode_dib(&bytes)?;
                return Some(Data::Image(w, h, std::sync::Arc::new(rgba)));
            }
            None
        })();
        let _ = CloseClipboard();
        data
    }
}

unsafe fn put(format: u32, bytes: &[u8]) -> bool {
    unsafe {
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
        let p = GlobalLock(g) as *mut u8;
        if p.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(g);
        SetClipboardData(format, Some(HANDLE(g.0))).is_ok()
    }
}

/// Copia de volta (texto, arquivos ou imagem). Não gera item novo no histórico.
pub fn copy(data: &Data) -> bool {
    unsafe {
        if !open(HWND::default()) {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = match data {
            Data::Text(t) => {
                let wide: Vec<u16> = t.encode_utf16().chain(Some(0)).collect();
                put(CF_UNICODETEXT.0 as u32, std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2))
            }
            Data::Files(files) => {
                // DROPFILES + lista de caminhos UTF-16 terminada em dois zeros.
                let mut list: Vec<u16> = Vec::new();
                for f in files {
                    list.extend(f.as_os_str().encode_wide_vec());
                    list.push(0);
                }
                list.push(0);
                let header = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, fWide: true.into(), ..Default::default() };
                let mut bytes = std::slice::from_raw_parts(&header as *const _ as *const u8, std::mem::size_of::<DROPFILES>()).to_vec();
                bytes.extend(std::slice::from_raw_parts(list.as_ptr() as *const u8, list.len() * 2));
                put(CF_HDROP.0 as u32, &bytes)
            }
            Data::Image(w, h, rgba) => put(CF_DIB.0 as u32, &nocto_core::imgconv::encode_dib(*w, *h, rgba)),
        };
        let _ = CloseClipboard();
        OWN_SEQ.store(GetClipboardSequenceNumber(), Ordering::Relaxed);
        ok
    }
}

trait WideVec {
    fn encode_wide_vec(&self) -> Vec<u16>;
}
impl WideVec for std::ffi::OsStr {
    fn encode_wide_vec(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}
