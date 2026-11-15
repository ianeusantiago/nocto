//! Integração fina com o Win32: atalho global, instância única, posição/forma/vidro
//! da janela e detecção de app em tela cheia. Tudo orientado a eventos, sem polling.

use nocto_core::keys::Hotkey;
use std::cell::RefCell;
use std::ffi::c_void;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;

use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

pub fn hwnd_of(window: &slint::Window) -> Option<isize> {
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window
        .with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        })
        .flatten()
}

fn h(raw: isize) -> HWND {
    HWND(raw as *mut c_void)
}

/// Data e hora locais: (ano, mês, dia, hora, minuto, segundo).
pub fn local_time() -> (i32, u32, u32, u32, u32, u32) {
    let t = unsafe { GetLocalTime() };
    (t.wYear as i32, t.wMonth as u32, t.wDay as u32, t.wHour as u32, t.wMinute as u32, t.wSecond as u32)
}

/// Primeira instância: devolve true e chama `on_signal` sempre que outra instância for aberta.
/// Instância repetida: avisa a primeira e devolve false.
pub fn single_instance(on_signal: impl Fn() + Send + 'static) -> bool {
    unsafe {
        let Ok(event) = CreateEventW(None, false, false, w!("Local\\NoctoShowBar")) else {
            return true;
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = SetEvent(event);
            return false;
        }
        let raw = event.0 as isize;
        std::thread::spawn(move || {
            while WaitForSingleObject(HANDLE(raw as *mut c_void), INFINITE) == WAIT_OBJECT_0 {
                on_signal();
            }
        });
        true
    }
}

/// Registra o atalho global numa thread própria que dorme em GetMessageW.
pub fn spawn_hotkey(hk: Hotkey, on_press: impl Fn() + Send + 'static) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || unsafe {
        let r = RegisterHotKey(None, 1, HOT_KEY_MODIFIERS(hk.modifiers) | MOD_NOREPEAT, hk.vk);
        let ok = r.is_ok();
        let _ = tx.send(r.map_err(|e| e.message()));
        if !ok {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_HOTKEY {
                on_press();
            }
        }
    });
    rx.recv().unwrap_or_else(|_| Err("falha ao registrar o atalho".into()))
}

/// Esconde da barra de tarefas e do Alt+Tab. O vidro é feito em glass.rs.
pub fn setup(raw: isize) {
    unsafe {
        let hwnd = h(raw);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize));
    }
}

/// Janela fixa, colada no topo do monitor e centralizada. É posicionada uma única vez:
/// depois disso só a ilha muda de forma dentro dela (nada de redimensionar a janela).
pub fn place_fixed(raw: isize, width: f32, height: f32, scale: f32) {
    unsafe {
        let hwnd = h(raw);
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let pw = (width * scale).round() as i32;
        let ph = (height * scale).round() as i32;
        let m = mi.rcMonitor;
        let x = m.left + ((m.right - m.left) - pw) / 2;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, m.top, pw, ph, SWP_NOACTIVATE);
    }
}

/// Área clicável = só a ilha (pixels físicos). O resto da janela deixa o clique passar
/// para o que está atrás. A borda visível é desenhada pelo compositor (suave); esta
/// região fica 2 px por fora, só para o clique.
pub fn set_region(raw: isize, x: f32, width: f32, height: f32, radius: f32) {
    unsafe {
        let pad = 2;
        let d = (radius * 2.0).round() as i32 + pad * 2;
        let rgn = CreateRoundRectRgn(
            x.round() as i32 - pad,
            -d,
            (x + width).round() as i32 + pad + 1,
            height.round() as i32 + pad + 1,
            d,
            d,
        );
        // Sem "repintar": o Windows apagaria o fundo da janela com branco e o Slint só
        // redesenha a ilha (apareciam a caixa e as barras brancas).
        SetWindowRgn(h(raw), Some(rgn), false);
    }
}

/// Área visível vazia: a janela continua existindo (com o desenho guardado), mas
/// não aparece nem pega cliques.
pub fn clear_region(raw: isize) {
    unsafe {
        SetWindowRgn(h(raw), Some(CreateRectRgn(0, 0, 0, 0)), false);
    }
}

pub fn show(raw: isize, visible: bool) {
    unsafe {
        let _ = ShowWindow(h(raw), if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn activate(raw: isize) {
    unsafe {
        let _ = SetForegroundWindow(h(raw));
    }
}

/// A janela é de um programa de captura de tela?
fn is_capture_tool(hwnd: HWND) -> bool {
    const TOOLS: [&str; 7] = [
        "screenclippinghost.exe",
        "snippingtool.exe",
        "sharex.exe",
        "lightshot.exe",
        "greenshot.exe",
        "flameshot.exe",
        "screensketch.exe",
    ];
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        TOOLS.iter().any(|t| path.ends_with(t))
    }
}

/// O app em primeiro plano ocupa o monitor inteiro (jogo, vídeo em tela cheia)?
pub fn is_fullscreen(raw: isize) -> bool {
    unsafe {
        let fg = h(raw);
        if fg.is_invalid() || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return false;
        }
        let mut class = [0u16; 32];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if class == "WorkerW" || class == "Progman" {
            return false;
        }
        // Janela maximizada não é tela cheia (com a barra de tarefas oculta, ela também
        // cobre o monitor inteiro e escondia a ilha sem motivo).
        if IsZoomed(fg).as_bool() {
            return false;
        }
        // Sobreposições de tela inteira que não são jogo (ferramenta de print, por
        // exemplo): a ilha sumia justamente do print.
        let ex = GetWindowLongPtrW(fg, GWL_EXSTYLE);
        if ex & WS_EX_TOOLWINDOW.0 as isize != 0 || is_capture_tool(fg) {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

thread_local! {
    static ON_FOREGROUND: RefCell<Option<Box<dyn Fn(isize)>>> = RefCell::new(None);
}

unsafe extern "system" fn foreground_changed(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    ON_FOREGROUND.with(|f| {
        if let Ok(f) = f.try_borrow() {
            if let Some(f) = f.as_ref() {
                f(hwnd.0 as isize);
            }
        }
    });
}

/// Avisa quando outro app vem para a frente. Hook fora de processo: sem injeção de DLL
/// e entregue pela fila de mensagens desta thread (a da interface).
pub fn watch_foreground(f: impl Fn(isize) + 'static) {
    ON_FOREGROUND.with(|slot| *slot.borrow_mut() = Some(Box::new(f)));
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    }
}
