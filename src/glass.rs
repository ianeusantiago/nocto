//! O vidro da ilha (estilo Liquid Glass), desenhado pelo compositor do Windows atrás da
//! interface do Slint. O compositor recorta com bordas suaves (antisserrilhadas) e roda as
//! animações de mola sozinho, sem a CPU do Nocto.
//!
//! Camadas, de baixo para cima, todas recortadas pelo mesmo retângulo arredondado:
//! desfoque do que está atrás → cor do horário → brilho (luz de cima e de baixo) → borda de luz.

use windows_numerics::{Vector2, Vector3};
use windows::Foundation::TimeSpan;
use windows::System::DispatcherQueueController;
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, Interface, Result, w};

/// A parte de cima do retângulo fica escondida acima da janela: assim os cantos de cima
/// somem (a ilha "gruda" no teto) e só os de baixo aparecem arredondados.
const HIDDEN_TOP: f32 = 48.0;

/// Duração do morph da ilha (o Slint anima o conteúdo no mesmo tempo).
pub const MORPH_MS: i64 = 420;

pub struct Glass {
    /// Janela própria do vidro, logo atrás da janela da interface (o compositor desenharia
    /// por cima do Slint se ficasse na mesma janela).
    pub hwnd: isize,
    _queue: DispatcherQueueController,
    _target: DesktopWindowTarget,
    compositor: Compositor,
    island: ContainerVisual,
    geometry: CompositionRoundedRectangleGeometry,
    tint: CompositionColorBrush,
    blur: SpriteVisual,
}

fn color(argb: [u8; 4]) -> Color {
    Color { A: argb[0], R: argb[1], G: argb[2], B: argb[3] }
}

fn ms(v: i64) -> TimeSpan {
    TimeSpan { Duration: v * 10_000 }
}

unsafe extern "system" fn glass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Cria a janela do vidro e põe a janela da interface como "dona" dela: o Windows mantém
/// a interface sempre por cima do vidro.
fn create_window(ui_hwnd: HWND) -> Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("NoctoGlass");
        let wc = WNDCLASSW { lpfnWndProc: Some(glass_proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        SetWindowLongPtrW(ui_hwnd, GWLP_HWNDPARENT, hwnd.0 as isize);
        Ok(hwnd)
    }
}

impl Glass {
    pub fn new(ui_hwnd: isize) -> Result<Self> {
        let hwnd = create_window(HWND(ui_hwnd as *mut core::ffi::c_void))?;
        let queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?
        };
        // Windows 11: libera o desfoque do que está atrás da janela. No 10, falha sem problema.
        unsafe {
            let on = BOOL(1);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(17), // DWMWA_USE_HOSTBACKDROPBRUSH
                &on as *const _ as *const _,
                std::mem::size_of::<BOOL>() as u32,
            );
        }
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false)? };

        let root = compositor.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        target.SetRoot(&root)?;

        let island = compositor.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&island)?;

        let geometry = compositor.CreateRoundedRectangleGeometry()?;
        geometry.SetOffset(Vector2 { X: 0.0, Y: -HIDDEN_TOP })?;
        // O tamanho do recorte acompanha o da ilha (mais a parte escondida), inclusive na mola.
        let follow = compositor.CreateExpressionAnimationWithExpression(&HSTRING::from(format!(
            "Vector2(island.Size.X, island.Size.Y + {HIDDEN_TOP})"
        )))?;
        follow.SetReferenceParameter(&HSTRING::from("island"), &island)?;
        geometry.StartAnimation(&HSTRING::from("Size"), &follow)?;
        island.SetClip(&compositor.CreateGeometricClipWithGeometry(&geometry)?)?;

        let layer = |brush: &CompositionBrush| -> Result<SpriteVisual> {
            let sprite = compositor.CreateSpriteVisual()?;
            sprite.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
            sprite.SetBrush(brush)?;
            island.Children()?.InsertAtTop(&sprite)?;
            Ok(sprite)
        };

        // 1. Desfoque do que está atrás (sem suporte, vira cor sólida). Com opacidade
        // menor que 1, parte do fundo aparece nítido: o vidro fica mais "líquido".
        let blur = layer(&compositor.CreateHostBackdropBrush()?.cast()?)?;

        // 2. Cor do horário.
        let tint = compositor.CreateColorBrushWithColor(color([150, 11, 42, 68]))?;
        layer(&tint.cast()?)?;

        // 3. Brilho: luz suave descendo do topo e uma faixa de luz junto à borda de baixo.
        let sheen = compositor.CreateLinearGradientBrush()?;
        sheen.SetStartPoint(Vector2 { X: 0.0, Y: 0.0 })?;
        sheen.SetEndPoint(Vector2 { X: 0.0, Y: 1.0 })?;
        let stops = sheen.ColorStops()?;
        for (offset, argb) in [
            (0.0, [46, 255, 255, 255]),
            (0.45, [0, 255, 255, 255]),
            (0.82, [0, 255, 255, 255]),
            (1.0, [30, 255, 255, 255]),
        ] {
            stops.Append(&compositor.CreateColorGradientStopWithOffsetAndColor(offset, color(argb))?)?;
        }
        layer(&sheen.cast()?)?;

        // 4. Borda de luz (mais forte embaixo, onde a ilha "pousa").
        let rim_brush = compositor.CreateLinearGradientBrush()?;
        rim_brush.SetStartPoint(Vector2 { X: 0.0, Y: 0.0 })?;
        rim_brush.SetEndPoint(Vector2 { X: 0.0, Y: 1.0 })?;
        let stops = rim_brush.ColorStops()?;
        for (offset, argb) in [(0.0, [20, 255, 255, 255]), (0.7, [60, 255, 255, 255]), (1.0, [150, 255, 255, 255])] {
            stops.Append(&compositor.CreateColorGradientStopWithOffsetAndColor(offset, color(argb))?)?;
        }
        let rim = compositor.CreateSpriteShapeWithGeometry(&geometry)?;
        rim.SetStrokeBrush(&rim_brush)?;
        rim.SetStrokeThickness(1.5)?;
        let shapes = compositor.CreateShapeVisual()?;
        shapes.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        shapes.Shapes()?.Append(&rim)?;
        island.Children()?.InsertAtTop(&shapes)?;

        Ok(Self { hwnd: hwnd.0 as isize, _queue: queue, _target: target, compositor, island, geometry, tint, blur })
    }

    /// Leva a ilha para a posição/tamanho/raio (pixels físicos). Com `animate`, usa uma
    /// mola parecida com a da Dynamic Island: rápida, com um leve "quique".
    pub fn set_shape(&self, x: f32, width: f32, height: f32, radius: f32, animate: bool) -> Result<()> {
        let size = Vector2 { X: width, Y: height };
        let offset = Vector3 { X: x, Y: 0.0, Z: 0.0 };
        let corner = Vector2 { X: radius.min(HIDDEN_TOP), Y: radius.min(HIDDEN_TOP) };
        if !animate {
            self.island.SetSize(size)?;
            self.island.SetOffset(offset)?;
            return self.geometry.SetCornerRadius(corner);
        }
        // Curva com leve passagem do ponto (overshoot), imitando a mola da Dynamic Island.
        let easing = self.compositor.CreateCubicBezierEasingFunction(
            Vector2 { X: 0.25, Y: 1.28 },
            Vector2 { X: 0.32, Y: 1.0 },
        )?;
        let v2 = |value: Vector2| -> Result<Vector2KeyFrameAnimation> {
            let a = self.compositor.CreateVector2KeyFrameAnimation()?;
            a.InsertKeyFrameWithEasingFunction(1.0, value, &easing)?;
            a.SetDuration(ms(MORPH_MS))?;
            Ok(a)
        };
        let v3 = self.compositor.CreateVector3KeyFrameAnimation()?;
        v3.InsertKeyFrameWithEasingFunction(1.0, offset, &easing)?;
        v3.SetDuration(ms(MORPH_MS))?;
        self.island.StartAnimation(&HSTRING::from("Size"), &v2(size)?)?;
        self.island.StartAnimation(&HSTRING::from("Offset"), &v3)?;
        self.geometry.StartAnimation(&HSTRING::from("CornerRadius"), &v2(corner)?)?;
        Ok(())
    }

    /// Quanto do desfoque aparece (1 = fosco; menor = mais transparente).
    pub fn set_blur(&self, opacity: f32) -> Result<()> {
        self.blur.SetOpacity(opacity)
    }

    /// Troca a cor do vidro com um fade lento (ARGB).
    pub fn set_tint(&self, argb: [u8; 4], fade_ms: i64) -> Result<()> {
        if fade_ms == 0 {
            return self.tint.SetColor(color(argb));
        }
        let a = self.compositor.CreateColorKeyFrameAnimation()?;
        a.InsertKeyFrame(1.0, color(argb))?;
        a.SetDuration(ms(fade_ms))?;
        self.tint.StartAnimation(&HSTRING::from("Color"), &a)
    }
}
