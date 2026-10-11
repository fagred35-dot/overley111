//! M0 research-спайк Overley: прозрачное click-through окно поверх игры.
//!
//! Проверяет ровно то, что нужно для решения по стеку (docs/M0-decision.md):
//!   M0-1  видно ли наше окно поверх borderless-игры, не мерцает ли;
//!   M0-2  влияние на FPS (замеряется ВНЕ этого бинаря, CapFrameX/PresentMon);
//!   M0-3  UpdateLayeredWindow как рабочий путь (DComp/D2D появится на M1);
//!   M0-4  подмена системного курсора + восстановление после `taskkill /F`;
//!   M0-6  долетают ли RegisterHotKey во время игры;
//!   +     задержка cursor-режима (QPC-метки вокруг GetPhysicalCursorPos → UpdateLayeredWindow).
//!
//! Ничего не инжектим, не читаем чужую память, не трогаем ввод: только своё окно,
//! GetForegroundWindow/GetWindowRect и GetPhysicalCursorPos.
//!
//! Сборка и запуск — только на Windows:
//!   cargo build --release
//!   target\release\overlay-win.exe ..\..\presets\examples\classic-cross.ovcross.json
//!   overlay-win.exe preset.json --cursor            # прицел следует за мышью
//!   overlay-win.exe preset.json --cursor-blank      # + подменить системный курсор (эксперимент M0-4)
//!   overlay-win.exe --restore-cursor                # вернуть курсор после сбоя
//!
//! Win32 здесь объявлен прямым FFI намеренно (независимость от версии крейта `windows`);
//! на M1 переезжаем на `windows` + DirectComposition (docs/ARCHITECTURE.md §3).

mod preset;
mod raster;

use preset::{Mode, Preset};
use raster::Frame;

#[cfg(not(windows))]
fn main() {
    eprintln!("Спайк overlay-win собирается и запускается только на Windows.");
    eprintln!("Протокол замеров: spike/README.md. Семантику DSL можно посмотреть без Windows:");
    eprintln!("  python tools/render_preview.py presets/examples/*.json");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(windows)]
mod win {
    use super::{Frame, Mode, Preset};
    use std::ffi::c_void;

    // --------------------------------------------------------------- FFI (только спайк)
    // FIXME-spike: сигнатуры выписаны вручную под x64 Windows. Если линкер ругнётся —
    // сверь имена с Win32-докой (все функции Ansi/W-пар: берём *W). На M1 заменяется крейтом `windows`.

    type HWND = isize;
    type HDC = isize;
    type HBITMAP = isize;
    type HCURSOR = isize;
    type WPARAM = usize;
    type LPARAM = isize;
    type LRESULT = isize;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Size {
        cx: i32,
        cy: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Msg {
        hwnd: HWND,
        message: u32,
        wParam: WPARAM,
        lParam: LPARAM,
        time: u32,
        pt: Point,
    }
    #[repr(C)]
    struct WndClassExW {
        cb_size: u32,
        style: u32,
        lpfn_wnd_proc: Option<extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>,
        cb_cls_extra: i32,
        cb_wnd_extra: i32,
        h_instance: isize,
        h_icon: isize,
        h_cursor: isize,
        hbr_background: isize,
        lpsz_menu_name: *const u16,
        lpsz_class_name: *const u16,
        h_icon_sm: isize,
    }
    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels: i32,
        bi_y_pels: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }
    #[repr(C)]
    struct BitmapInfo {
        header: BitmapInfoHeader,
        colors: [u32; 1],
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct BlendFunction {
        blend_op: u8,
        blend_flags: u8,
        source_constant_alpha: u8,
        alpha_format: u8,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
        fn QueryPerformanceCounter(v: *mut i64) -> i32;
        fn QueryPerformanceFrequency(v: *mut i64) -> i32;
        fn GetCurrentProcessId() -> u32;
    }
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(ms: u32) -> u32;
        fn timeEndPeriod(ms: u32) -> u32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn CreateCompatibleDC(hdc: HDC) -> HDC;
        fn DeleteDC(hdc: HDC) -> i32;
        fn SelectObject(hdc: HDC, obj: isize) -> isize;
        fn DeleteObject(obj: isize) -> i32;
        fn CreateDIBSection(
            hdc: HDC, bmi: *const BitmapInfo, usage: u32, bits: *mut *mut u8, section: isize, offset: u32,
        ) -> HBITMAP;
    }
    #[link(name = "user32")]
    extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
        fn RegisterClassExW(cls: *const WndClassExW) -> u16;
        fn CreateWindowExW(
            ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32,
            parent: HWND, menu: isize, instance: isize, param: *mut c_void,
        ) -> HWND;
        fn DestroyWindow(hwnd: HWND) -> i32;
        fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
        fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn DefWindowProcW(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT;
        fn GetMessageW(msg: *mut Msg, hwnd: HWND, min: u32, max: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> LRESULT;
        fn PostQuitMessage(code: i32);
        fn RegisterHotKey(hwnd: HWND, id: i32, modifiers: u32, vk: u32) -> i32;
        fn UnregisterHotKey(hwnd: HWND, id: i32) -> i32;
        fn SetTimer(hwnd: HWND, id: usize, ms: u32, proc: Option<extern "system" fn(HWND, u32, usize, u32)>) -> usize;
        fn UpdateLayeredWindow(
            hwnd: HWND, hdc_dst: HDC, dst: *const Point, size: *const Size, hdc_src: HDC, src: *const Point,
            key: u32, blend: *const BlendFunction, flags: u32,
        ) -> i32;
        fn GetForegroundWindow() -> HWND;
        fn GetWindowRect(hwnd: HWND, rect: *mut Rect) -> i32;
        fn GetPhysicalCursorPos(p: *mut Point) -> i32;
        fn CreateCursor(
            inst: isize, x_hot: i32, y_hot: i32, w: i32, h: i32, and_plane: *const u8, xor_plane: *const u8,
        ) -> HCURSOR;
        fn SetSystemCursor(hcur: HCURSOR, id: u32) -> i32;
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, win_ini: u32) -> i32;
    }

    const WS_EX_LAYERED: u32 = 0x0008_0000;
    const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_POPUP: u32 = 0x8000_0000;
    const ULW_ALPHA: u32 = 0x0000_0002;
    const WM_DESTROY: u32 = 0x0002;
    const WM_TIMER: u32 = 0x0113;
    const WM_HOTKEY: u32 = 0x0312;
    const SW_HIDE: i32 = 0;
    const HWND_TOPMOST: HWND = -1;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const MOD_NOREPEAT: u32 = 0x4000;
    const VK_F7: u32 = 0x76;
    const VK_F8: u32 = 0x77;
    const VK_F9: u32 = 0x78;
    const SPI_SETCURSORS: u32 = 0x0057;
    const OCR_NORMAL: u32 = 32512;
    const OCR_IBEAM: u32 = 32513;
    const OCR_CROSS: u32 = 32515;
    const OCR_SIZEALL: u32 = 32646;
    const OCR_SIZENWSE: u32 = 32642;
    const OCR_SIZENESW: u32 = 32643;
    const OCR_SIZEWE: u32 = 32644;
    const OCR_SIZENS: u32 = 32645;
    const OCR_NO: u32 = 32648;
    const OCR_HAND: u32 = 32649;
    const OCR_APPSTARTING: u32 = 32650;
    const DIB_RGB_COLORS: u32 = 0;
    const BI_RGB: u32 = 0;
    const DPI_PER_MONITOR_AWARE_V2: isize = -3;

    // --------------------------------------------------------------- состояние

    struct State {
        hwnd: HWND,
        mode: Mode,
        offset: [f32; 2],
        frame: Frame,
        mem_dc: HDC,
        old_obj: isize,
        visible: bool,
        cursor_blank: bool,
        last_x: i32,
        last_y: i32,
        samples: Vec<f64>, // задержки input→present в мкс (cursor-режим)
        qpc_freq: f64,
        fg_rect: Rect,
    }

    // FIXME-spike: глобальный мутабельный указатель допустим только в спайке.
    // В M1 состояние живёт в структуре ядра и передаётся через GWLP_USERDATA или Arc.

    fn state() -> &'static mut State {
        unsafe { &mut *STATE }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn now_us(freq: f64) -> i64 {
        let mut v = 0i64;
        unsafe { QueryPerformanceCounter(&mut v) };
        ((v as f64 / freq) * 1e6) as i64
    }

    // --------------------------------------------------------------- оверлей

    fn present(st: &mut State, x: i32, y: i32) {
        let dst = Point { x, y };
        let size = Size { cx: st.frame.width as i32, cy: st.frame.height as i32 };
        let src = Point { x: 0, y: 0 };
        let blend = BlendFunction { blend_op: 0, blend_flags: 0, source_constant_alpha: 255, alpha_format: 1 };
        let ok = unsafe {
            UpdateLayeredWindow(st.hwnd, 0, &dst, &size, st.mem_dc, &src, 0, &blend, ULW_ALPHA)
        };
        if ok == 0 {
            eprintln!("  ! UpdateLayeredWindow вернул 0 — проверь, что окно не скрыто и DC жив");
        }
        st.last_x = x;
        st.last_y = y;
    }

    fn target_pos(st: &mut State) -> (i32, i32) {
        let (w, h) = (st.frame.width as i32 / 2, st.frame.height as i32 / 2);
        match st.mode {
            Mode::Cursor => {
                let mut p = Point::default();
                unsafe { GetPhysicalCursorPos(&mut p) };
                (p.x - w, p.y - h)
            }
            Mode::Center => {
                let fg = unsafe { GetForegroundWindow() };
                let mut r = Rect::default();
                if fg != 0 && unsafe { GetWindowRect(fg, &mut r) } != 0 {
                    st.fg_rect = r;
                }
                let r = st.fg_rect;
                let cx = (r.left + r.right) / 2;
                let cy = (r.top + r.bottom) / 2;
                (cx - w + st.offset[0] as i32, cy - h + st.offset[1] as i32)
            }
        }
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_HOTKEY => {
                let st = state();
                match wp as i32 {
                    1 => {
                        // F7: показать/спрятать (паника)
                        st.visible = !st.visible;
                        if st.visible {
                            let (x, y) = target_pos(st);
                            present(st, x, y);
                            unsafe { ShowWindow(st.hwnd, 4) };
                        } else {
                            unsafe { ShowWindow(st.hwnd, SW_HIDE) };
                        }
                        println!("[spike] оверлей {}", if st.visible { "ПОКАЗАН" } else { "СКРЫТ" });
                    }
                    2 => {
                        // F8: выйти (и восстановить курсор)
                        unsafe { PostQuitMessage(0) };
                    }
                    3 => {
                        // F9: переключить center/cursor на лету
                        st.mode = match st.mode {
                            Mode::Center => Mode::Cursor,
                            Mode::Cursor => Mode::Center,
                        };
                        println!("[spike] режим: {:?}", st.mode);
                        let (x, y) = target_pos(st);
                        present(st, x, y);
                    }
                    _ => {}
                }
                0
            }
            WM_TIMER => {
                let st = state();
                let t0 = now_us(st.qpc_freq);
                let (x, y) = target_pos(st);
                if st.visible && (x != st.last_x || y != st.last_y || st.mode == Mode::Cursor) {
                    present(st, x, y);
                    let dt = now_us(st.qpc_freq) - t0;
                    st.samples.push(dt as f64);
                    if st.samples.len() >= 200_000 {
                        st.samples.drain(0..100_000);
                    }
                }
                0
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }

    // --------------------------------------------------------------- курсор (M0-4)

    fn lock_path() -> std::path::PathBuf {
        let app = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        let dir = std::path::Path::new(&app).join("Overley-spike");
        std::fs::create_dir_all(&dir).ok();
        dir.join("cursor.lock")
    }

    fn blank_the_cursor() -> bool {
        let planes = [0u8; 32 * 32 / 8];
        let cur = unsafe { CreateCursor(0, 0, 0, 32, 32, planes.as_ptr(), planes.as_ptr()) };
        if cur == 0 {
            eprintln!("  ! CreateCursor не удался — курсор не трогаем");
            return false;
        }
        for id in [
            OCR_NORMAL, OCR_IBEAM, OCR_CROSS, OCR_SIZEALL, OCR_SIZENWSE, OCR_SIZENESW,
            OCR_SIZEWE, OCR_SIZENS, OCR_NO, OCR_HAND, OCR_APPSTARTING,
        ] {
            unsafe { SetSystemCursor(cur, id) };
        }
        std::fs::write(lock_path(), format!("{}\n", unsafe { GetCurrentProcessId() })).ok();
        println!("[spike] системный курсор подменён на пустой (lock-файл создан)");
        true
    }

    pub fn restore_cursor() {
        unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), 0) };
        std::fs::remove_file(lock_path()).ok();
        println!("[spike] системные курсоры восстановлены (SPI_SETCURSORS), lock-файл удалён");
    }

    // --------------------------------------------------------------- запуск

    pub fn run() {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--restore-cursor") {
            restore_cursor();
            return;
        }

        let mut preset_path = None;
        let mut force_mode = None;
        let mut cursor_blank = false;
        let mut out_scale = 2.0f32;
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--cursor" => force_mode = Some(Mode::Cursor),
                "--center" => force_mode = Some(Mode::Center),
                "--cursor-blank" => {
                    cursor_blank = true;
                    force_mode = force_mode.or(Some(Mode::Center));
                }
                "--scale" => {
                    i += 1;
                    out_scale = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(2.0);
                }
                other if !other.starts_with("--") => preset_path = Some(other.to_string()),
                _ => {}
            }
            i += 1;
        }
        let preset_path = preset_path
            .unwrap_or_else(|| "presets/examples/classic-cross.ovcross.json".to_string());

        let preset: Preset = match crate::preset::load(&preset_path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("не загрузился пресет: {}", e.0);
                std::process::exit(2);
            }
        };
        let mode = force_mode.unwrap_or(preset.mode);
        println!(
            "[spike] пресет {:?}: слоёв {}, холст {:?}, режим {:?}",
            preset_path, preset.layers.len(), preset.canvas, mode
        );

        unsafe { SetProcessDpiAwarenessContext(DPI_PER_MONITOR_AWARE_V2) };
        unsafe { timeBeginPeriod(1) };

        let frame = crate::raster::render(&preset, out_scale);
        println!("[spike] растр {}x{} (масштаб {out_scale})", frame.width, frame.height);

        // DIB + memory DC
        let bmi = BitmapInfo {
            header: BitmapInfoHeader {
                bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: frame.width as i32,
                bi_height: -(frame.height as i32), // top-down
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: BI_RGB,
                bi_size_image: 0,
                bi_x_pels: 0,
                bi_y_pels: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            colors: [0],
        };
        let mut bits: *mut u8 = std::ptr::null_mut();
        let mem_dc = unsafe { CreateCompatibleDC(0) };
        let dib = unsafe { CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0) };
        if dib == 0 || bits.is_null() {
            eprintln!("не удалось создать DIB section");
            std::process::exit(2);
        }
        unsafe { std::ptr::copy_nonoverlapping(frame.pixels.as_ptr(), bits, frame.pixels.len()) };
        let old_obj = unsafe { SelectObject(mem_dc, dib) };

        // окно
        let class = wide("OverleySpikeOverlay");
        let title = wide("Overley spike");
        let wc = WndClassExW {
            cb_size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            lpfn_wnd_proc: Some(wnd_proc),
            cb_cls_extra: 0,
            cb_wnd_extra: 0,
            h_instance: unsafe { GetModuleHandleW(std::ptr::null()) },
            h_icon: 0,
            h_cursor: 0,
            hbr_background: 0,
            lpsz_menu_name: std::ptr::null(),
            lpsz_class_name: class.as_ptr(),
            h_icon_sm: 0,
        };
        unsafe { RegisterClassExW(&wc) };
        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        let hwnd = unsafe {
            CreateWindowExW(
                ex,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                -1000,
                -1000,
                frame.width as i32,
                frame.height as i32,
                0,
                0,
                wc.h_instance,
                std::ptr::null_mut(),
            )
        };
        if hwnd == 0 {
            eprintln!("не удалось создать окно оверлея");
            std::process::exit(2);
        }

        let mut freq = 0i64;
        unsafe { QueryPerformanceFrequency(&mut freq) };
        let st = Box::leak(Box::new(State {
            hwnd,
            mode,
            offset: preset.offset,
            frame,
            mem_dc,
            old_obj,
            visible: true,
            cursor_blank: false,
            last_x: i32::MIN,
            last_y: i32::MIN,
            samples: Vec::with_capacity(4096),
            qpc_freq: freq.max(1) as f64,
            fg_rect: Rect::default(),
        }));
        unsafe { STATE = st as *mut State };;

        if cursor_blank {
            state().cursor_blank = blank_the_cursor();
        }

        unsafe { RegisterHotKey(hwnd, 1, MOD_NOREPEAT, VK_F7) };
        unsafe { RegisterHotKey(hwnd, 2, MOD_NOREPEAT, VK_F8) };
        unsafe { RegisterHotKey(hwnd, 3, MOD_NOREPEAT, VK_F9) };
        println!("[spike] хоткеи: F7 показать/скрыть · F8 выйти · F9 центр/курсор");

        unsafe { SetTimer(hwnd, 1, 1, None) };
        let (x, y) = target_pos(state());
        present(state(), x, y);
        unsafe { ShowWindow(hwnd, 4) };
        unsafe { SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };

        println!("[spike] оверлей активен. Закрой игру/окно и проверь, что прицел остался поверх.");

        let mut msg = Msg { hwnd: 0, message: 0, wParam: 0, lParam: 0, time: 0, pt: Point::default() };
        loop {
            let r = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
            if r <= 0 {
                break;
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // сводка по задержкам (cursor-режим)
        let st = state();
        if !st.samples.is_empty() {
            let mut s = st.samples.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let q = |p: f64| s[(p * (s.len() - 1) as f64) as usize];
            println!(
                "[spike] задержка input→present (мкс): p50={:.0} p95={:.0} max={:.0} (n={})",
                q(0.5), q(0.95), s[s.len() - 1], s.len()
            );
        }
        unsafe {
            UnregisterHotKey(hwnd, 1);
            UnregisterHotKey(hwnd, 2);
            UnregisterHotKey(hwnd, 3);
            SelectObject(mem_dc, old_obj);
            DeleteObject(dib);
            DeleteDC(mem_dc);
            DestroyWindow(hwnd);
            timeEndPeriod(1);
        }
        if st.cursor_blank {
            restore_cursor();
        }
        println!("[spike] завершено");
    }
}
