//! The window as the whole screen of a text console, without X11 or Wayland.
//!
//! SDL2 gives the OpenGL context: its KMSDRM driver sets the mode of the screen and shows what
//! is drawn. The library is loaded when the program runs, like the X11 libraries of the back
//! end beside this one. Where SDL2 has no keys of its own, they are those of the terminal of
//! the program, which also ends a program that is busy (see `console`).

#![allow(non_snake_case)]

pub mod console;
pub mod terminal;

use crate::{
    event::{EventHandler, KeyCode, KeyMods, MouseButton},
    native::{Clipboard, NativeDisplayData, Request},
};
use console::Console;
use std::ffi::{c_void, CStr, CString};
use std::os::raw::{c_char, c_int};
use std::time::Instant;

const SDL_INIT_VIDEO: u32 = 0x20;
const SDL_HINT_OVERRIDE: c_int = 2;
const SDL_WINDOW_OPENGL: u32 = 0x2;
const SDL_WINDOW_RESIZABLE: u32 = 0x20;
const SDL_WINDOW_FULLSCREEN_DESKTOP: u32 = 0x1001;
const SDL_WINDOW_ALLOW_HIGHDPI: u32 = 0x2000;
const SDL_WINDOWPOS_UNDEFINED: c_int = 0x1fff_0000;
const SDL_GL_RED_SIZE: c_int = 0;
const SDL_GL_GREEN_SIZE: c_int = 1;
const SDL_GL_BLUE_SIZE: c_int = 2;
const SDL_GL_ALPHA_SIZE: c_int = 3;
const SDL_GL_DOUBLEBUFFER: c_int = 5;
const SDL_GL_DEPTH_SIZE: c_int = 6;
const SDL_GL_STENCIL_SIZE: c_int = 7;
const SDL_GL_MULTISAMPLEBUFFERS: c_int = 13;
const SDL_GL_MULTISAMPLESAMPLES: c_int = 14;
const SDL_GL_CONTEXT_MAJOR_VERSION: c_int = 17;
const SDL_GL_CONTEXT_MINOR_VERSION: c_int = 18;
const SDL_GL_CONTEXT_PROFILE_MASK: c_int = 21;
const SDL_GL_CONTEXT_PROFILE_COMPATIBILITY: c_int = 2;
const SDL_QUIT: u32 = 0x100;
const SDL_WINDOWEVENT: u32 = 0x200;
const SDL_KEYDOWN: u32 = 0x300;
const SDL_KEYUP: u32 = 0x301;
const SDL_TEXTINPUT: u32 = 0x303;
const SDL_MOUSEMOTION: u32 = 0x400;
const SDL_MOUSEBUTTONDOWN: u32 = 0x401;
const SDL_MOUSEBUTTONUP: u32 = 0x402;
const SDL_MOUSEWHEEL: u32 = 0x403;
const SDL_WINDOWEVENT_SIZE_CHANGED: u8 = 6;
const SDL_WINDOWEVENT_MINIMIZED: u8 = 7;
const SDL_WINDOWEVENT_RESTORED: u8 = 9;
const SDL_WINDOWEVENT_CLOSE: u8 = 14;
const SDL_MOUSEWHEEL_FLIPPED: u32 = 1;
const KMOD_SHIFT: u16 = 0x0003;
const KMOD_CTRL: u16 = 0x00c0;
const KMOD_ALT: u16 = 0x0300;
const KMOD_GUI: u16 = 0x0c00;

#[repr(C)]
#[derive(Clone, Copy)]
struct KeyboardEvent {
    kind: u32,
    timestamp: u32,
    window: u32,
    state: u8,
    repeat: u8,
    padding: [u8; 2],
    scancode: c_int,
    sym: i32,
    modifiers: u16,
    unused: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TextInputEvent {
    kind: u32,
    timestamp: u32,
    window: u32,
    text: [c_char; 32],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MouseMotionEvent {
    kind: u32,
    timestamp: u32,
    window: u32,
    which: u32,
    state: u32,
    x: i32,
    y: i32,
    xrel: i32,
    yrel: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MouseButtonEvent {
    kind: u32,
    timestamp: u32,
    window: u32,
    which: u32,
    button: u8,
    state: u8,
    clicks: u8,
    padding: u8,
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MouseWheelEvent {
    kind: u32,
    timestamp: u32,
    window: u32,
    which: u32,
    x: i32,
    y: i32,
    direction: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WindowEvent {
    kind: u32,
    timestamp: u32,
    window: u32,
    event: u8,
    padding: [u8; 3],
    data1: i32,
    data2: i32,
}

/// `SDL_Event`: 56 bytes that are aligned like a pointer.
#[repr(C)]
union Event {
    kind: u32,
    key: KeyboardEvent,
    text: TextInputEvent,
    motion: MouseMotionEvent,
    button: MouseButtonEvent,
    wheel: MouseWheelEvent,
    window: WindowEvent,
    size: [u64; 7],
}

crate::declare_module! {
    LibSdl2,
    "libSDL2-2.0.so.0",
    "libSDL2.so",
    ...
    ...
    fn SDL_InitSubSystem(u32) -> c_int,
    fn SDL_QuitSubSystem(u32),
    fn SDL_Quit(),
    fn SDL_GetError() -> *const c_char,
    fn SDL_SetHintWithPriority(*const c_char, *const c_char, c_int) -> c_int,
    fn SDL_GL_ResetAttributes(),
    fn SDL_GL_SetAttribute(c_int, c_int) -> c_int,
    fn SDL_CreateWindow(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void,
    fn SDL_DestroyWindow(*mut c_void),
    fn SDL_GL_CreateContext(*mut c_void) -> *mut c_void,
    fn SDL_GL_DeleteContext(*mut c_void),
    fn SDL_GL_SetSwapInterval(c_int) -> c_int,
    fn SDL_GL_SwapWindow(*mut c_void),
    fn SDL_GL_GetProcAddress(*const c_char) -> Option<unsafe extern "C" fn()>,
    fn SDL_GL_GetDrawableSize(*mut c_void, *mut c_int, *mut c_int),
    fn SDL_PollEvent(*mut Event) -> c_int,
    fn SDL_ShowCursor(c_int) -> c_int,
    fn SDL_SetWindowGrab(*mut c_void, c_int),
    fn SDL_SetWindowSize(*mut c_void, c_int, c_int),
    fn SDL_SetWindowPosition(*mut c_void, c_int, c_int),
    fn SDL_SetWindowFullscreen(*mut c_void, u32) -> c_int,
    fn SDL_Delay(u32),
    ...
    ...
}

/// The name of SDL2's driver for the screen of a text console.
const KMSDRM: &str = "KMSDRM";

/// Whether the window is the screen of the text console: when the program says so, and when
/// there is no X11 or Wayland display to open a window on.
pub fn wanted(conf: &crate::conf::Conf) -> bool {
    let set = |name| std::env::var_os(name).map_or(false, |value| !value.is_empty());
    conf.platform.linux_console || !(set("DISPLAY") || set("WAYLAND_DISPLAY"))
}

/// SDL2's video driver: the one of the text console unless `SDL_VIDEODRIVER` names another.
fn driver() -> Result<String, String> {
    match std::env::var("SDL_VIDEODRIVER") {
        Ok(name) if name.contains(',') => Err(
            "SDL_VIDEODRIVER must name one driver: the text console cannot be a second choice"
                .to_owned(),
        ),
        Ok(name) if name.eq_ignore_ascii_case(KMSDRM) => Ok(KMSDRM.to_owned()),
        Ok(name) if !name.is_empty() => Ok(name),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err("SDL_VIDEODRIVER is not the name of a driver".to_owned())
        }
        _ => Ok(KMSDRM.to_owned()),
    }
}

/// Whether the user may read the input devices of Linux, from which SDL2 takes the keys and
/// the mouse of a text console.
fn input_devices() -> bool {
    std::fs::read_dir("/dev/input").map_or(false, |devices| {
        devices.flatten().any(|device| {
            device.file_name().to_string_lossy().starts_with("event")
                && std::fs::File::open(device.path()).is_ok()
        })
    })
}

fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

/// SDL2's video, until this is dropped.
struct Video(&'static LibSdl2);

impl Video {
    fn start(sdl: &'static LibSdl2, driver: &str, own_signals: bool) -> Result<Self, String> {
        let hint = |name: &str, value: &str| unsafe {
            let (name, value) = (CString::new(name).unwrap(), CString::new(value).unwrap());
            (sdl.SDL_SetHintWithPriority)(name.as_ptr(), value.as_ptr(), SDL_HINT_OVERRIDE);
        };
        hint("SDL_VIDEODRIVER", driver);
        if own_signals {
            hint("SDL_NO_SIGNAL_HANDLERS", "1");
        }
        if unsafe { (sdl.SDL_InitSubSystem)(SDL_INIT_VIDEO) } != 0 {
            let reason = text(unsafe { (sdl.SDL_GetError)() });
            unsafe { (sdl.SDL_Quit)() };
            let needs = if driver == KMSDRM {
                ": it needs a screen with kernel mode setting that no X server is using, and \
                 the right to open /dev/dri/card*"
            } else {
                ""
            };
            return Err(format!(
                "SDL2 cannot open its {driver} video ({reason}){needs}"
            ));
        }
        Ok(Self(sdl))
    }

    fn error(&self) -> String {
        text(unsafe { (self.0.SDL_GetError)() })
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        unsafe {
            (self.0.SDL_QuitSubSystem)(SDL_INIT_VIDEO);
            (self.0.SDL_Quit)();
        }
    }
}

/// A window with its OpenGL context, until this is dropped.
struct Window<'a> {
    video: &'a Video,
    window: *mut c_void,
    context: *mut c_void,
}

impl<'a> Window<'a> {
    /// The window that `conf` describes, or the whole screen where SDL2's driver has no
    /// windows. `samples`: of its multisampling, 1 for none.
    fn open(
        video: &'a Video,
        conf: &crate::conf::Conf,
        screen: bool,
        samples: i32,
    ) -> Result<Self, String> {
        let sdl = video.0;
        let attribute = |name, value| unsafe { (sdl.SDL_GL_SetAttribute)(name, value) };
        unsafe { (sdl.SDL_GL_ResetAttributes)() };
        // What the X11 back end asks GLX for.
        attribute(SDL_GL_RED_SIZE, 8);
        attribute(SDL_GL_GREEN_SIZE, 8);
        attribute(SDL_GL_BLUE_SIZE, 8);
        attribute(SDL_GL_ALPHA_SIZE, 8);
        attribute(SDL_GL_DEPTH_SIZE, 24);
        attribute(SDL_GL_STENCIL_SIZE, 8);
        attribute(SDL_GL_DOUBLEBUFFER, 1);
        attribute(SDL_GL_MULTISAMPLEBUFFERS, i32::from(samples > 1));
        attribute(
            SDL_GL_MULTISAMPLESAMPLES,
            if samples > 1 { samples } else { 0 },
        );
        attribute(
            SDL_GL_CONTEXT_PROFILE_MASK,
            SDL_GL_CONTEXT_PROFILE_COMPATIBILITY,
        );
        attribute(SDL_GL_CONTEXT_MAJOR_VERSION, 2);
        attribute(SDL_GL_CONTEXT_MINOR_VERSION, 1);
        let mut flags = SDL_WINDOW_OPENGL;
        if screen || conf.fullscreen {
            flags |= SDL_WINDOW_FULLSCREEN_DESKTOP;
        }
        if conf.window_resizable {
            flags |= SDL_WINDOW_RESIZABLE;
        }
        if conf.high_dpi {
            flags |= SDL_WINDOW_ALLOW_HIGHDPI;
        }
        let title = CString::new(conf.window_title.replace('\0', " ")).unwrap();
        let window = unsafe {
            (sdl.SDL_CreateWindow)(
                title.as_ptr(),
                SDL_WINDOWPOS_UNDEFINED,
                SDL_WINDOWPOS_UNDEFINED,
                conf.window_width,
                conf.window_height,
                flags,
            )
        };
        if window.is_null() {
            return Err(format!("SDL2 cannot open the window: {}", video.error()));
        }
        let mut opened = Self {
            video,
            window,
            context: std::ptr::null_mut(),
        };
        opened.context = unsafe { (sdl.SDL_GL_CreateContext)(window) };
        if opened.context.is_null() {
            return Err(format!(
                "SDL2 cannot make the OpenGL context: {}",
                video.error()
            ));
        }
        Ok(opened)
    }

    fn size(&self) -> (i32, i32) {
        let (mut width, mut height) = (0, 0);
        unsafe { (self.video.0.SDL_GL_GetDrawableSize)(self.window, &mut width, &mut height) };
        (width, height)
    }
}

impl Drop for Window<'_> {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_null() {
                (self.video.0.SDL_GL_DeleteContext)(self.context);
            }
            (self.video.0.SDL_DestroyWindow)(self.window);
        }
    }
}

/// The text that the program copies stays in the program: a text console has no clipboard.
struct Copied(Option<String>);

impl Clipboard for Copied {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }

    fn set(&mut self, string: &str) {
        self.0 = Some(string.to_owned());
    }
}

fn mods(modifiers: u16) -> KeyMods {
    KeyMods {
        shift: modifiers & KMOD_SHIFT != 0,
        ctrl: modifiers & KMOD_CTRL != 0,
        alt: modifiers & KMOD_ALT != 0,
        logo: modifiers & KMOD_GUI != 0,
    }
}

/// The key of an SDL2 key event: by what the key types where it types something, as the
/// X11 back end has it, and by its place on the keyboard otherwise.
fn translate(sym: i32, scancode: c_int) -> KeyCode {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    const DIGITS: [KeyCode; 10] = [Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9];
    const FUNCTIONS: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    const KEYPAD: [KeyCode; 10] = [Kp1, Kp2, Kp3, Kp4, Kp5, Kp6, Kp7, Kp8, Kp9, Kp0];
    match sym {
        0x61..=0x7a => return LETTERS[(sym - 0x61) as usize],
        0x30..=0x39 => return DIGITS[(sym - 0x30) as usize],
        0x20 => return Space,
        0x27 => return Apostrophe,
        0x2c => return Comma,
        0x2d => return Minus,
        0x2e => return Period,
        0x2f => return Slash,
        0x3b => return Semicolon,
        0x3d => return Equal,
        0x5b => return LeftBracket,
        0x5c => return Backslash,
        0x5d => return RightBracket,
        0x60 => return GraveAccent,
        0x1b => return Escape,
        0x0d => return Enter,
        0x09 => return Tab,
        0x08 => return Backspace,
        0x7f => return Delete,
        _ => {}
    }
    match scancode {
        58..=69 => FUNCTIONS[(scancode - 58) as usize],
        89..=98 => KEYPAD[(scancode - 89) as usize],
        57 => CapsLock,
        70 => PrintScreen,
        71 => ScrollLock,
        72 => Pause,
        73 => Insert,
        74 => Home,
        75 => PageUp,
        76 => Delete,
        77 => End,
        78 => PageDown,
        79 => Right,
        80 => Left,
        81 => Down,
        82 => Up,
        83 => NumLock,
        84 => KpDivide,
        85 => KpMultiply,
        86 => KpSubtract,
        87 => KpAdd,
        88 => KpEnter,
        99 => KpDecimal,
        101 => Menu,
        103 => KpEqual,
        224 => LeftControl,
        225 => LeftShift,
        226 => LeftAlt,
        227 => LeftSuper,
        228 => RightControl,
        229 => RightShift,
        230 => RightAlt,
        231 => RightSuper,
        _ => Unknown,
    }
}

/// Whether the key is one of those that switch consoles with Alt.
fn function(key: KeyCode) -> bool {
    use KeyCode::*;
    matches!(
        key,
        F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12
    )
}

/// Lets the program see that it is asked to end, and ends it when it does not refuse.
fn quit_if_requested(event_handler: &mut dyn EventHandler) {
    let requested = {
        let display = crate::native_display().lock().unwrap();
        display.quit_requested && !display.quit_ordered
    };
    if requested {
        event_handler.quit_requested_event();
        let mut display = crate::native_display().lock().unwrap();
        if display.quit_requested {
            display.quit_ordered = true;
        }
    }
}

fn request_quit() {
    crate::native_display().lock().unwrap().quit_requested = true;
}

/// Runs the program in a window of SDL2 until it ends. Everything of SDL2 is gone when this
/// returns, also after a panic of the program.
fn show<F>(
    sdl: &'static LibSdl2,
    driver: &str,
    conf: &crate::conf::Conf,
    f: &mut Option<F>,
    mut console: Option<&mut Console>,
) -> Result<(), String>
where
    F: 'static + FnOnce() -> Box<dyn EventHandler>,
{
    let screen = driver == KMSDRM;
    // Whether the keys are what is typed on the terminal, not those that SDL2 tells.
    let typed = console.as_ref().map_or(false, |console| console.typed());
    let video = Video::start(sdl, driver, console.is_some())?;
    // A driver that cannot smooth the edges still shows the picture.
    let window = Window::open(&video, conf, screen, conf.sample_count).or_else(|error| {
        if conf.sample_count > 1 {
            Window::open(&video, conf, screen, 1)
        } else {
            Err(error)
        }
    })?;
    unsafe {
        (sdl.SDL_GL_SetSwapInterval)(conf.platform.swap_interval.unwrap_or(1));
        if screen {
            (sdl.SDL_ShowCursor)(0);
        }
    }
    crate::native::gl::load_gl_funcs(|name| {
        let name = CString::new(name).unwrap();
        unsafe { (sdl.SDL_GL_GetProcAddress)(name.as_ptr()) }
    });
    let (width, height) = window.size();
    let (requests, received) = std::sync::mpsc::channel();
    crate::set_display(NativeDisplayData {
        high_dpi: conf.high_dpi,
        blocking_event_loop: conf.platform.blocking_event_loop,
        ..NativeDisplayData::new(width, height, requests, Box::new(Copied(None)))
    });
    let mut update_requested = true;
    let mut event_handler = (f.take().unwrap())();
    // What SDL2 told as held and the program got, and where the mouse is.
    let (mut keys, mut buttons) = (Vec::<KeyCode>::new(), Vec::<MouseButton>::new());
    let mut pointer = (0.0, 0.0);

    while !crate::native_display().lock().unwrap().quit_ordered {
        while let Ok(request) = received.try_recv() {
            unsafe {
                match request {
                    Request::ScheduleUpdate => update_requested = true,
                    Request::SetCursorGrab(grab) => {
                        (sdl.SDL_SetWindowGrab)(window.window, c_int::from(grab))
                    }
                    Request::ShowMouse(shown) => {
                        (sdl.SDL_ShowCursor)(c_int::from(shown));
                    }
                    Request::SetWindowSize {
                        new_width,
                        new_height,
                    } => (sdl.SDL_SetWindowSize)(window.window, new_width as _, new_height as _),
                    Request::SetWindowPosition { new_x, new_y } => {
                        (sdl.SDL_SetWindowPosition)(window.window, new_x as _, new_y as _)
                    }
                    Request::SetFullscreen(fullscreen) if !screen => {
                        let flags = if fullscreen {
                            SDL_WINDOW_FULLSCREEN_DESKTOP
                        } else {
                            0
                        };
                        (sdl.SDL_SetWindowFullscreen)(window.window, flags);
                    }
                    // The screen of a console is always full, and has one cursor.
                    Request::SetFullscreen(_)
                    | Request::SetMouseCursor(_)
                    | Request::ShowKeyboard(_) => {}
                }
            }
        }

        // SDL2 tells the keys and the mouse of the input devices also while the user is at
        // another console, where they are not meant for this program: there nothing is held.
        let here = console.as_ref().map_or(true, |console| console.shown());
        if !here {
            for key in keys.drain(..) {
                event_handler.key_up_event(key, KeyMods::default());
            }
            for button in buttons.drain(..) {
                event_handler.mouse_button_up_event(button, pointer.0, pointer.1);
            }
        }
        let mut event = Event { size: [0; 7] };
        while unsafe { (sdl.SDL_PollEvent)(&mut event) } != 0 {
            // Each kind of event is read as what SDL2 wrote for it.
            match unsafe { event.kind } {
                SDL_QUIT => request_quit(),
                SDL_WINDOWEVENT => match unsafe { event.window.event } {
                    SDL_WINDOWEVENT_SIZE_CHANGED => {
                        let (width, height) = window.size();
                        let resized = {
                            let mut display = crate::native_display().lock().unwrap();
                            let resized =
                                (display.screen_width, display.screen_height) != (width, height);
                            display.screen_width = width;
                            display.screen_height = height;
                            resized
                        };
                        if resized {
                            event_handler.resize_event(width as _, height as _);
                        }
                    }
                    SDL_WINDOWEVENT_MINIMIZED => event_handler.window_minimized_event(),
                    SDL_WINDOWEVENT_RESTORED => event_handler.window_restored_event(),
                    SDL_WINDOWEVENT_CLOSE => request_quit(),
                    _ => {}
                },
                SDL_KEYDOWN | SDL_KEYUP if !typed && here => {
                    let key = unsafe { event.key };
                    let code = translate(key.sym, key.scancode);
                    let mods = mods(key.modifiers);
                    // On a console the kernel has these keys too: Control with C is the
                    // signal of the terminal, and Alt with a function key switches consoles.
                    if console.is_some()
                        && ((mods.ctrl && code == KeyCode::C) || (mods.alt && function(code)))
                    {
                        continue;
                    }
                    keys.retain(|held| *held != code);
                    if key.kind == SDL_KEYDOWN {
                        keys.push(code);
                        event_handler.key_down_event(code, mods, key.repeat != 0);
                    } else {
                        event_handler.key_up_event(code, mods);
                    }
                }
                SDL_TEXTINPUT if !typed && here => {
                    let typed = text(unsafe { event.text.text.as_ptr() });
                    for character in typed.chars() {
                        event_handler.char_event(character, KeyMods::default(), false);
                    }
                }
                SDL_MOUSEMOTION if here => {
                    let motion = unsafe { event.motion };
                    pointer = (motion.x as f32, motion.y as f32);
                    event_handler.mouse_motion_event(motion.x as _, motion.y as _);
                    event_handler.raw_mouse_motion(motion.xrel as _, motion.yrel as _);
                }
                SDL_MOUSEBUTTONDOWN | SDL_MOUSEBUTTONUP if here => {
                    let button = unsafe { event.button };
                    let which = match button.button {
                        1 => MouseButton::Left,
                        2 => MouseButton::Middle,
                        3 => MouseButton::Right,
                        _ => MouseButton::Unknown,
                    };
                    let (x, y) = (button.x as f32, button.y as f32);
                    pointer = (x, y);
                    buttons.retain(|held| *held != which);
                    match (which, button.kind) {
                        (MouseButton::Unknown, _) => {}
                        (_, SDL_MOUSEBUTTONDOWN) => {
                            buttons.push(which);
                            event_handler.mouse_button_down_event(which, x, y)
                        }
                        _ => event_handler.mouse_button_up_event(which, x, y),
                    }
                }
                SDL_MOUSEWHEEL if here => {
                    let wheel = unsafe { event.wheel };
                    let sign = if wheel.direction == SDL_MOUSEWHEEL_FLIPPED {
                        -1.0
                    } else {
                        1.0
                    };
                    event_handler.mouse_wheel_event(sign * wheel.x as f32, sign * wheel.y as f32);
                }
                _ => {}
            }
        }

        if let Some(console) = console.as_mut() {
            for event in console.events(Instant::now())? {
                match event {
                    console::Event::Down(stroke, repeat) => {
                        // Control with C ends a program in a terminal.
                        if stroke.key == KeyCode::C && stroke.mods.ctrl {
                            request_quit();
                            continue;
                        }
                        if stroke.key != KeyCode::Unknown {
                            event_handler.key_down_event(stroke.key, stroke.mods, repeat);
                        }
                        if let Some(character) = stroke.character {
                            event_handler.char_event(character, stroke.mods, repeat);
                        }
                    }
                    console::Event::Up(key, mods) => event_handler.key_up_event(key, mods),
                }
            }
            // A signal, or the user who switches to another console.
            if console.stopping() {
                request_quit();
            }
        }
        quit_if_requested(&mut *event_handler);

        if !conf.platform.blocking_event_loop || update_requested {
            update_requested = false;
            event_handler.update();
            event_handler.draw();
            unsafe { (sdl.SDL_GL_SwapWindow)(window.window) };
        } else {
            unsafe { (sdl.SDL_Delay)(5) };
        }
    }
    if let Some(console) = console.as_ref() {
        console.stand_down();
    }
    // The program lets go of what it drew while the context is there.
    drop(event_handler);
    Ok(())
}

/// Runs the program with the screen of the text console as its window.
pub fn run<F>(conf: &crate::conf::Conf, f: &mut Option<F>) -> Result<(), String>
where
    F: 'static + FnOnce() -> Box<dyn EventHandler>,
{
    open(conf, f).map_err(|error| {
        if conf.platform.linux_console {
            error
        } else {
            format!("there is no X11 display (DISPLAY is not set), and the text console cannot be used instead: {error}")
        }
    })
}

fn open<F>(conf: &crate::conf::Conf, f: &mut Option<F>) -> Result<(), String>
where
    F: 'static + FnOnce() -> Box<dyn EventHandler>,
{
    let driver = driver()?;
    // Never unloaded: SDL2 may leave handlers behind that run when the program ends.
    let sdl: &'static LibSdl2 = Box::leak(Box::new(LibSdl2::try_load().map_err(|_| {
        "the text console needs SDL2: its library libSDL2-2.0.so.0 was not found".to_owned()
    })?));
    let screen = driver == KMSDRM;
    // In a window system the keys and the signals are those of every window.
    let windowed = matches!(driver.as_str(), "x11" | "wayland");
    // On Linux SDL2 has the keys and the mouse of a console from the input devices.
    let devices = cfg!(target_os = "linux") && screen && input_devices();
    // The screen of a FreeBSD console is taken with its terminal or not at all.
    let owned = cfg!(target_os = "freebsd") && screen;
    let mut console = if windowed {
        None
    } else {
        Console::open(owned, !devices)?
    };
    if let Some(console) = console.as_ref() {
        // SDL2 must leave the keyboard to the terminal. Nothing else of the program runs yet.
        std::env::set_var("SDL_INPUT_FREEBSD_KEEP_KBD", "1");
        std::env::set_var("SDL_INPUT_LINUX_KEEP_KBD", "1");
        console.watch();
    } else if !windowed && !devices {
        eprintln!(
            "No keyboard: the input devices cannot be read and the program has no terminal. \
             A signal ends it."
        );
    }
    let shown = show(sdl, &driver, conf, f, console.as_mut());
    // Only now may the console let a switch happen.
    match console {
        Some(console) => shown.and(console.close()),
        None => shown,
    }
}
