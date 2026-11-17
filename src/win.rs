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
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
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

/// O winit esconde a moldura da janela, mas mantém por baixo os estilos de barra de
/// título e borda. Ao ganhar ou perder o foco, o Windows repintava essa barra (branca,
/// com "Nocto" e X) por um instante. Aqui essa pintura é bloqueada.
unsafe extern "system" fn no_frame_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_NCPAINT => LRESULT(0),
            // lParam = -1: o Windows atualiza o estado de ativo/inativo sem repintar a moldura.
            WM_NCACTIVATE => DefSubclassProc(hwnd, msg, wp, LPARAM(-1)),
            _ => DefSubclassProc(hwnd, msg, wp, lp),
        }
    }
}

/// Esconde da barra de tarefas e do Alt+Tab e bloqueia a pintura da moldura padrão.
/// O vidro é feito em glass.rs.
pub fn setup(raw: isize) {
    unsafe {
        let hwnd = h(raw);
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize));
        let _ = SetWindowSubclass(hwnd, Some(no_frame_proc), 1, 0);
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

/// A janela da frente cobre o topo do meio da tela (onde fica a pílula)?
/// Ex.: navegador maximizado, com as abas embaixo da pílula.
pub fn covers_top(raw: isize) -> bool {
    unsafe {
        let fg = h(raw);
        if fg.is_invalid() || fg == GetShellWindow() || fg == GetDesktopWindow() || IsIconic(fg).as_bool() {
            return false;
        }
        let mut class = [0u16; 32];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if class == "WorkerW" || class == "Progman" || class == "Shell_TrayWnd" {
            return false;
        }
        let ex = GetWindowLongPtrW(fg, GWL_EXSTYLE);
        if ex & WS_EX_TOOLWINDOW.0 as isize != 0 || is_capture_tool(fg) {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mid = GetSystemMetrics(SM_CXSCREEN) / 2;
        r.top <= 40 && r.bottom > 40 && r.left < mid + 180 && r.right > mid - 180
    }
}

/// O mouse está encostado no topo, no meio da tela (para espiar a pílula escondida)?
/// `reach` é a altura da faixa que conta, em pixels.
pub fn cursor_at_top(reach: i32) -> bool {
    unsafe {
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_err() {
            return false;
        }
        let mid = GetSystemMetrics(SM_CXSCREEN) / 2;
        p.y <= reach && (p.x - mid).abs() < 220
    }
}

/// %APPDATA% (Roaming) perguntado ao Windows.
pub fn roaming_appdata() -> Option<std::path::PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
        s.map(std::path::PathBuf::from)
    }
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// Traz a janela para a frente com o foco do teclado, mesmo vindo de outro app.
/// O Windows costuma recusar SetForegroundWindow de quem não está na frente; ligar
/// por um instante a entrada da thread da janela da frente à nossa resolve.
pub fn activate(raw: isize) {
    unsafe {
        let hwnd = h(raw);
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = windows::Win32::System::Threading::GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        // Uma "tecla" vazia faz o Windows considerar que este processo recebeu a última
        // entrada do usuário, condição para poder trazer a janela para a frente.
        let nudge = INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0x87), ..Default::default() } } };
        let mut up = nudge;
        up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        SendInput(&[nudge, up], std::mem::size_of::<INPUT>() as i32);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
}

/// Liga/desliga o Nocto junto com o Windows (chave "Run" do usuário, sem admin).
pub fn set_autostart(on: bool) {
    use windows::Win32::System::Registry::*;
    unsafe {
        let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let value: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(Some(0)).collect();
            let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, w!("Nocto"), REG_SZ.0, Some(bytes.as_ptr() as _), bytes.len() as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, w!("Nocto"));
        }
    }
}

/// Nome do executável de um processo, em minúsculas (ex.: "spotify.exe").
pub fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        Some(path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string())
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
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    process_name(pid).is_some_and(|n| TOOLS.contains(&n.as_str()))
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

/// Caminho de arquivo de onde tirar o ícone de um item da lista do "abre":
/// o próprio arquivo, um .exe achado no PATH ("notepad.exe") ou, para "spotify:",
/// o Spotify instalado. `None` quando não há arquivo (ex.: "ms-settings:").
pub fn icon_source(target: &str) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    let lower = target.to_lowercase();
    if lower.ends_with(".lnk") {
        // Ícone do programa para onde o atalho aponta (sem a setinha de atalho).
        if let Some(exe) = shortcut_target(Path::new(target)) {
            return Some(exe);
        }
    }
    if Path::new(target).is_file() {
        return Some(PathBuf::from(target));
    }
    if lower.starts_with("spotify:") {
        let exe = PathBuf::from(std::env::var_os("APPDATA")?).join("Spotify").join("Spotify.exe");
        return exe.is_file().then_some(exe);
    }
    if lower.ends_with(".exe") {
        let name: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buf = [0u16; 520];
        let len = unsafe {
            windows::Win32::Storage::FileSystem::SearchPathW(
                windows::core::PCWSTR::null(),
                windows::core::PCWSTR(name.as_ptr()),
                windows::core::PCWSTR::null(),
                Some(&mut buf),
                None,
            )
        } as usize;
        if len > 0 && len < buf.len() {
            return Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
        }
    }
    None
}

/// Ícone grande (32×32) de um arquivo, em RGBA, pelo próprio Windows.
pub fn file_icon_rgba(path: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
    unsafe {
        let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            windows::core::PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let icon = info.hIcon;
        let mut ii = ICONINFO::default();
        let result = (|| {
            GetIconInfo(icon, &mut ii).ok()?;
            let dc = CreateCompatibleDC(None);
            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            // Primeiro pergunta o tamanho do bitmap do ícone.
            GetDIBits(dc, ii.hbmColor, 0, 0, None, &mut bmi, DIB_RGB_COLORS);
            let (w, h) = (bmi.bmiHeader.biWidth.unsigned_abs(), bmi.bmiHeader.biHeight.unsigned_abs());
            if w == 0 || h == 0 || w > 256 || h > 256 {
                let _ = DeleteDC(dc);
                return None;
            }
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB.0;
            bmi.bmiHeader.biHeight = -(h as i32); // de cima para baixo
            let mut px = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(dc, ii.hbmColor, 0, h, Some(px.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
            let _ = DeleteDC(dc);
            if lines == 0 {
                return None;
            }
            // BGRA → RGBA; ícones antigos vêm sem alfa: ficam opacos.
            let has_alpha = px.chunks(4).any(|p| p[3] != 0);
            for p in px.chunks_mut(4) {
                p.swap(0, 2);
                if !has_alpha {
                    p[3] = 255;
                }
            }
            Some((w, h, px))
        })();
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }
        let _ = DestroyIcon(icon);
        result
    }
}

/// Para onde um atalho .lnk aponta (se for um arquivo que existe).
fn shortcut_target(lnk: &std::path::Path) -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::Interface;
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        let wide: Vec<u16> = lnk.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        file.Load(windows::core::PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 520];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len]));
        path.is_file().then_some(path)
    }
}
