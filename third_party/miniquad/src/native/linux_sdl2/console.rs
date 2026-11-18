//! The terminal of the program as the keyboard of its window and, on FreeBSD, the ownership
//! of the text console while the window is its screen.
//!
//! SDL2 has no keys on a FreeBSD console, and on Linux only for a user who may read the input
//! devices. The terminal has them for everyone. It is read in raw mode, and it gets its
//! settings back when the window closes, also when a signal stops the program or it crashes.
//!
//! A program that calculates for minutes does not look at its keys meanwhile. Control with C
//! stays the signal of the terminal for that, and a watcher ends a program that was busy when
//! it was asked to end and still is a few seconds later: the terminal is given back first.
//!
//! On FreeBSD SDL2 does not let go of the screen when the user switches to another console,
//! which leaves that one dead. So the program takes the screen only in the foreground of the
//! active text console, owns the switching while it has the screen, and ends in order when
//! the user switches away: the switch happens when everything is given back.

use super::terminal::{Ended, Holds, Parser, Stroke};
use crate::event::{KeyCode, KeyMods};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const KD_TEXT: i32 = 0;
const KD_GRAPHICS: i32 = 1;
const K_XLATE: i32 = 1;
const VT_AUTO: u8 = 0;
const VT_PROCESS: u8 = 1;

/// `struct vt_mode` of FreeBSD's `sys/consio.h`: who switches the console.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
struct VtMode {
    mode: u8,
    waitv: u8,
    release: i16,
    acquire: i16,
    force: i16,
}

/// The ioctls of FreeBSD's `sys/consio.h` and `sys/kbio.h`.
#[cfg(target_os = "freebsd")]
mod request {
    use super::VtMode;

    const fn request<T>(direction: libc::c_ulong, group: u8, number: u8) -> libc::c_ulong {
        direction
            | ((std::mem::size_of::<T>() as libc::c_ulong) << 16)
            | ((group as libc::c_ulong) << 8)
            | number as libc::c_ulong
    }

    const READ: libc::c_ulong = 0x4000_0000;
    const WRITE: libc::c_ulong = 0x8000_0000;
    const VALUE: libc::c_ulong = 0x2000_0000;
    pub const KDGETMODE: libc::c_ulong = request::<i32>(READ, b'K', 9);
    pub const KDSETMODE: libc::c_ulong = request::<i32>(VALUE, b'K', 10);
    pub const KDGKBMODE: libc::c_ulong = request::<i32>(READ, b'K', 6);
    pub const KDSKBMODE: libc::c_ulong = request::<i32>(VALUE, b'K', 7);
    pub const KDGETREPEAT: libc::c_ulong = request::<[i32; 2]>(READ, b'K', 103);
    pub const VT_GETMODE: libc::c_ulong = request::<VtMode>(READ, b'v', 3);
    pub const VT_SETMODE: libc::c_ulong = request::<VtMode>(WRITE, b'v', 2);
    pub const VT_GETACTIVE: libc::c_ulong = request::<i32>(READ, b'v', 7);
    pub const VT_GETINDEX: libc::c_ulong = request::<i32>(READ, b'v', 8);
}

/// What the terminal was like when the program got it.
#[derive(Clone, Copy)]
struct Snapshot {
    attributes: libc::termios,
    /// Whether the program runs in the foreground of the terminal.
    foreground: bool,
    /// The number of the console that the terminal is, 0 if it is none, and of the active one.
    index: i32,
    active: i32,
    video: i32,
    keyboard: i32,
    switching: VtMode,
}

impl Snapshot {
    /// Whether the terminal is a text console whose screen the program may take.
    fn console(&self) -> Result<(), String> {
        if self.index <= 0 || self.index != self.active {
            return Err(format!(
                "the screen of the text console needs the active console as the terminal of the \
                 program, not SSH, tmux or a terminal emulator: this terminal is console {}, \
                 the active console is {}",
                self.index, self.active
            ));
        }
        if !self.foreground {
            return Err(
                "the screen of the text console needs the program in the foreground of its console"
                    .to_owned(),
            );
        }
        if self.video != KD_TEXT || self.keyboard != K_XLATE || self.switching.mode != VT_AUTO {
            return Err(
                "another program owns this console or draws on it: use an ordinary text console, \
                 not the one of Xorg"
                    .to_owned(),
            );
        }
        Ok(())
    }
}

/// The settings of the terminal that delivers every key at once and still ends a line of
/// output as a terminal does. Its interrupt key, Control with C, stays the signal that it is:
/// that reaches a program that is busy. The keys of the other signals are only keys.
fn raw(attributes: &libc::termios) -> libc::termios {
    let mut raw = *attributes;
    unsafe { libc::cfmakeraw(&mut raw) };
    raw.c_oflag |= libc::OPOST | libc::ONLCR;
    raw.c_lflag |= libc::ISIG;
    raw.c_cc[libc::VQUIT] = libc::_POSIX_VDISABLE;
    raw.c_cc[libc::VSUSP] = libc::_POSIX_VDISABLE;
    #[cfg(target_os = "freebsd")]
    {
        raw.c_cc[libc::VDSUSP] = libc::_POSIX_VDISABLE;
    }
    raw.c_cc[libc::VMIN] = 0;
    raw.c_cc[libc::VTIME] = 0;
    raw
}

/// What a terminal can be set to.
trait Device {
    fn attributes(&mut self, value: &libc::termios) -> Result<(), String>;
    fn active(&mut self) -> Result<i32, String>;
    fn switching(&mut self, value: VtMode) -> Result<(), String>;
    fn video(&mut self, value: i32) -> Result<(), String>;
    fn keyboard(&mut self, value: i32) -> Result<(), String>;
}

/// The terminal while the window has it. It gives back what it took, in the reverse order,
/// also when it is dropped by an error or a panic.
struct Guard<D: Device> {
    device: D,
    saved: Snapshot,
    owned: bool,
    raw: bool,
    graphics: bool,
    keyboard: bool,
}

impl<D: Device> Guard<D> {
    /// Sets the terminal to deliver every key at once. With `screen` the terminal is a text
    /// console whose screen the window takes: the program owns its switching first.
    fn enter(device: D, saved: Snapshot, screen: bool) -> Result<Self, String> {
        if screen {
            saved.console()?;
        }
        let mut guard = Self {
            device,
            saved,
            owned: false,
            raw: false,
            graphics: false,
            keyboard: false,
        };
        if screen {
            guard.device.switching(VtMode {
                mode: VT_PROCESS,
                release: libc::SIGUSR1 as i16,
                acquire: libc::SIGUSR2 as i16,
                ..VtMode::default()
            })?;
            guard.owned = true;
            guard.keyboard = true;
            if guard.device.active()? != guard.saved.index {
                return Err(
                    "the active console changed while the program started; the screen was not touched"
                        .to_owned(),
                );
            }
        }
        guard.device.attributes(&raw(&guard.saved.attributes))?;
        guard.raw = true;
        if screen {
            guard.device.video(KD_GRAPHICS)?;
            guard.graphics = true;
        }
        Ok(guard)
    }

    fn restore(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        let mut restore = |changed: &mut bool, result: Result<(), String>| match result {
            Ok(()) => *changed = false,
            Err(error) => errors.push(error),
        };
        if self.keyboard {
            restore(
                &mut self.keyboard,
                self.device.keyboard(self.saved.keyboard),
            );
        }
        if self.raw {
            restore(
                &mut self.raw,
                self.device.attributes(&self.saved.attributes),
            );
        }
        if self.graphics {
            restore(&mut self.graphics, self.device.video(self.saved.video));
        }
        // This lets a switch that waits happen: it is the last thing to give back.
        if self.owned {
            restore(&mut self.owned, self.device.switching(self.saved.switching));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "the terminal did not get everything back: {}",
                errors.join("; ")
            ))
        }
    }
}

impl<D: Device> Drop for Guard<D> {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("error: {error}");
        }
    }
}

/// The terminal that controls the program.
struct Terminal(File);

fn system(result: libc::c_int, what: &str) -> Result<(), String> {
    if result < 0 {
        Err(format!("{what}: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

impl Terminal {
    fn open() -> std::io::Result<Self> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open("/dev/tty")
            .map(Self)
    }

    fn snapshot(&self) -> Result<Snapshot, String> {
        let fd = self.0.as_raw_fd();
        let mut attributes = unsafe { std::mem::zeroed::<libc::termios>() };
        system(
            unsafe { libc::tcgetattr(fd, &mut attributes) },
            "reading the settings of the terminal",
        )?;
        let foreground = unsafe { libc::tcgetpgrp(fd) == libc::getpgrp() };
        let snapshot = Snapshot {
            attributes,
            foreground,
            index: 0,
            active: 0,
            video: KD_TEXT,
            keyboard: K_XLATE,
            switching: VtMode::default(),
        };
        // A terminal that is no console does not answer the first of these.
        #[cfg(target_os = "freebsd")]
        let snapshot = match self.read::<i32>(request::VT_GETINDEX) {
            Ok(index) => Snapshot {
                index,
                active: self.read(request::VT_GETACTIVE)?,
                video: self.read(request::KDGETMODE)?,
                keyboard: self.read(request::KDGKBMODE)?,
                switching: self.read(request::VT_GETMODE)?,
                ..snapshot
            },
            Err(_) => snapshot,
        };
        Ok(snapshot)
    }

    /// Whether the terminal is a virtual console of the kernel, not a terminal of SSH, tmux
    /// or a terminal emulator.
    fn virtual_console(&self) -> bool {
        // `VT_GETMODE`, which only a console answers.
        #[cfg(target_os = "freebsd")]
        let request = request::VT_GETMODE;
        #[cfg(not(target_os = "freebsd"))]
        let request: libc::c_ulong = 0x5601;
        let mut mode = VtMode::default();
        unsafe { libc::ioctl(self.0.as_raw_fd(), request as _, &mut mode) == 0 }
    }

    /// How long the keyboard of the console waits before it repeats a key, and between two
    /// repeats.
    fn rate(&self) -> Option<(Duration, Duration)> {
        #[cfg(target_os = "freebsd")]
        if let Ok([delay, repeat]) = self.read::<[i32; 2]>(request::KDGETREPEAT) {
            if delay > 0 && repeat > 0 {
                return Some((
                    Duration::from_millis(delay as u64),
                    Duration::from_millis(repeat as u64),
                ));
            }
        }
        None
    }

    #[cfg(target_os = "freebsd")]
    fn read<T: Default>(&self, request: libc::c_ulong) -> Result<T, String> {
        let mut value = T::default();
        system(
            unsafe { libc::ioctl(self.0.as_raw_fd(), request, &mut value) },
            "asking the console",
        )?;
        Ok(value)
    }

    /// Sets a mode of the FreeBSD console.
    #[cfg(target_os = "freebsd")]
    fn set(&self, request: libc::c_ulong, value: i32, what: &str) -> Result<(), String> {
        system(
            unsafe { libc::ioctl(self.0.as_raw_fd(), request, value as libc::c_ulong) },
            what,
        )
    }

    #[cfg(not(target_os = "freebsd"))]
    fn set(&self, _request: u32, _value: i32, what: &str) -> Result<(), String> {
        Err(format!("{what}: this is not a FreeBSD console"))
    }
}

#[cfg(not(target_os = "freebsd"))]
mod request {
    pub const KDSETMODE: u32 = 0;
    pub const KDSKBMODE: u32 = 0;
}

impl Device for Terminal {
    fn attributes(&mut self, value: &libc::termios) -> Result<(), String> {
        system(
            unsafe { libc::tcsetattr(self.0.as_raw_fd(), libc::TCSAFLUSH, value) },
            "setting the terminal",
        )
    }

    fn active(&mut self) -> Result<i32, String> {
        #[cfg(target_os = "freebsd")]
        return self.read(request::VT_GETACTIVE);
        #[cfg(not(target_os = "freebsd"))]
        Err("this is not a FreeBSD console".to_owned())
    }

    fn switching(&mut self, value: VtMode) -> Result<(), String> {
        #[cfg(target_os = "freebsd")]
        return system(
            unsafe { libc::ioctl(self.0.as_raw_fd(), request::VT_SETMODE, &value) },
            "setting who switches the console",
        );
        #[cfg(not(target_os = "freebsd"))]
        Err(format!("this is not a FreeBSD console ({value:?})"))
    }

    fn video(&mut self, value: i32) -> Result<(), String> {
        self.set(
            request::KDSETMODE,
            value,
            "setting the console to text or graphics",
        )
    }

    fn keyboard(&mut self, value: i32) -> Result<(), String> {
        self.set(
            request::KDSKBMODE,
            value,
            "setting the keyboard of the console",
        )
    }
}

/// Set by a signal that ends the program, and by the console when the user switches away.
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_signal: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// The signals that end the program: it must give the terminal back first.
const TERMINAL_SIGNALS: &[libc::c_int] = &[
    libc::SIGINT,
    libc::SIGTERM,
    libc::SIGHUP,
    libc::SIGQUIT,
    libc::SIGTSTP,
];
/// With the screen of the console also the two with which the console asks for a switch.
const CONSOLE_SIGNALS: &[libc::c_int] = &[
    libc::SIGUSR1,
    libc::SIGUSR2,
    libc::SIGINT,
    libc::SIGTERM,
    libc::SIGHUP,
    libc::SIGQUIT,
    libc::SIGTSTP,
];
/// The signals of a crash.
const CRASHES: [libc::c_int; 5] = [
    libc::SIGSEGV,
    libc::SIGBUS,
    libc::SIGILL,
    libc::SIGFPE,
    libc::SIGABRT,
];

fn handler(function: extern "C" fn(libc::c_int), flags: libc::c_int) -> libc::sigaction {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = function as usize;
    action.sa_flags = flags;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    action
}

/// Signals that set `STOP` instead of ending the program, until this is dropped.
struct Signals(Vec<(libc::c_int, libc::sigaction)>);

impl Signals {
    fn install(signals: &[libc::c_int]) -> Result<Self, String> {
        let mut installed = Self(Vec::new());
        for &signal in signals {
            let mut old = unsafe { std::mem::zeroed::<libc::sigaction>() };
            system(
                unsafe { libc::sigaction(signal, &handler(stop, libc::SA_RESTART), &mut old) },
                "installing a signal handler",
            )?;
            installed.0.push((signal, old));
        }
        Ok(installed)
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        for (signal, old) in &self.0 {
            unsafe { libc::sigaction(*signal, old, std::ptr::null_mut()) };
        }
    }
}

/// What a crash gives back to the terminal: nothing else of the program runs then.
struct Emergency {
    fd: libc::c_int,
    screen: bool,
    saved: Snapshot,
    /// The devices of the screens, which SDL2 has open while the window is a screen.
    screens: [libc::dev_t; SCREENS],
    old: [libc::sigaction; 5],
}

const SCREENS: usize = 8;
/// The descriptors among which those of the screen are looked for.
const DESCRIPTORS: libc::c_int = 4096;

static EMERGENCY: AtomicPtr<Emergency> = AtomicPtr::new(std::ptr::null_mut());

/// The devices of `/dev/dri/card*`, with which SDL2 shows the window on a screen.
fn screens() -> [libc::dev_t; SCREENS] {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let mut screens = [0; SCREENS];
    let cards = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|card| card.file_name().to_string_lossy().starts_with("card"))
        .filter_map(|card| std::fs::metadata(card.path()).ok())
        .filter(|card| card.file_type().is_char_device());
    for (screen, card) in screens.iter_mut().zip(cards) {
        *screen = card.rdev() as libc::dev_t;
    }
    screens
}

/// Gives the terminal back without the program's help. Only calls that a signal handler may
/// make. The screen of a console goes before its switching: the console that the user
/// switches to gets it then.
unsafe fn give_back(emergency: &Emergency) {
    #[cfg(target_os = "freebsd")]
    if emergency.screen {
        libc::ioctl(
            emergency.fd,
            request::KDSKBMODE,
            emergency.saved.keyboard as libc::c_ulong,
        );
    }
    libc::tcsetattr(emergency.fd, libc::TCSANOW, &emergency.saved.attributes);
    #[cfg(target_os = "freebsd")]
    if emergency.screen {
        libc::ioctl(
            emergency.fd,
            request::KDSETMODE,
            emergency.saved.video as libc::c_ulong,
        );
        for fd in 3..DESCRIPTORS {
            let mut status = std::mem::zeroed::<libc::stat>();
            if fd != emergency.fd
                && libc::fstat(fd, &mut status) == 0
                && status.st_mode & libc::S_IFMT == libc::S_IFCHR
                && status.st_rdev != 0
                && emergency.screens.contains(&status.st_rdev)
            {
                libc::close(fd);
            }
        }
        libc::ioctl(
            emergency.fd,
            request::VT_SETMODE,
            &emergency.saved.switching,
        );
    }
}

/// Gives the terminal back and lets the crash end the program.
extern "C" fn crash(signal: libc::c_int) {
    let emergency = EMERGENCY.swap(std::ptr::null_mut(), Ordering::SeqCst);
    unsafe {
        if !emergency.is_null() {
            give_back(&*emergency);
        }
        // The program must not go on with a terminal that it has given back.
        libc::signal(signal, libc::SIG_DFL);
        libc::raise(signal);
    }
}

/// When the program last looked at its console, in milliseconds of `now`.
static BEAT: AtomicU64 = AtomicU64::new(0);
/// The watcher that looks after the program, 0 when none does.
static WATCHER: AtomicU64 = AtomicU64::new(0);
/// A program that has not looked at its console for this long, in milliseconds, is busy.
const BUSY: u64 = if cfg!(test) { 300 } else { 1000 };
/// How long a program that was busy when it was asked to end may stay busy.
const PATIENCE: u64 = if cfg!(test) { 700 } else { 3000 };

/// The time of a clock that never goes back, in milliseconds.
fn now() -> u64 {
    let mut time = unsafe { std::mem::zeroed::<libc::timespec>() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    time.tv_sec as u64 * 1000 + time.tv_nsec as u64 / 1_000_000
}

/// What the watcher knows of a request to end the program.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Watch {
    /// Nothing asks the program to end.
    Calm,
    /// It was asked at `at`, when it had last looked at its console at `beat`, long before.
    Busy { at: u64, beat: u64 },
    /// It looked at its console when it was asked, or since: it ends by itself, also when
    /// that takes time, as the saving of a video does.
    Answering,
}

/// Whether the program must be ended without its help: it was busy when it was asked to
/// end, and it has not looked at its console since, for too long.
fn overdue(watch: &mut Watch, stopping: bool, beat: u64, now: u64) -> bool {
    *watch = match *watch {
        _ if !stopping => Watch::Calm,
        Watch::Calm if now.saturating_sub(beat) >= BUSY => Watch::Busy { at: now, beat },
        Watch::Busy { at, beat: seen } if beat == seen => {
            return now.saturating_sub(at) >= PATIENCE;
        }
        _ => Watch::Answering,
    };
    false
}

/// Ends a program that does not answer: the terminal gets back what a crash gives back.
fn abandon() {
    let emergency = EMERGENCY.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if emergency.is_null() {
        return;
    }
    const MESSAGE: &[u8] =
        b"error: the program was busy when it was asked to end, and it still is: \
        its console is given back and it ends now\n";
    unsafe {
        give_back(&*emergency);
        libc::write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
        libc::_exit(1);
    }
}

/// Starts the watcher of a program that has taken its terminal.
fn watch() {
    static WATCHERS: AtomicU64 = AtomicU64::new(0);
    let watcher = WATCHERS.fetch_add(1, Ordering::SeqCst) + 1;
    BEAT.store(now(), Ordering::SeqCst);
    WATCHER.store(watcher, Ordering::SeqCst);
    // Without the thread the program is as it would be without a watcher.
    let _ = std::thread::Builder::new()
        .name("console watcher".to_owned())
        .spawn(move || {
            let mut watch = Watch::Calm;
            while WATCHER.load(Ordering::SeqCst) == watcher {
                std::thread::sleep(Duration::from_millis(100));
                let stopping = STOP.load(Ordering::SeqCst);
                if overdue(&mut watch, stopping, BEAT.load(Ordering::SeqCst), now())
                    && WATCHER.load(Ordering::SeqCst) == watcher
                {
                    abandon();
                }
            }
        });
}

fn arm(fd: libc::c_int, screen: bool, saved: Snapshot) {
    let emergency = Box::into_raw(Box::new(Emergency {
        fd,
        screen,
        saved,
        screens: if screen { screens() } else { [0; SCREENS] },
        old: [unsafe { std::mem::zeroed::<libc::sigaction>() }; 5],
    }));
    EMERGENCY.store(emergency, Ordering::SeqCst);
    for (index, signal) in CRASHES.iter().enumerate() {
        unsafe {
            libc::sigaction(
                *signal,
                &handler(crash, libc::SA_ONSTACK),
                &mut (*emergency).old[index],
            );
        }
    }
}

fn disarm() {
    let emergency = EMERGENCY.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !emergency.is_null() {
        let emergency = unsafe { Box::from_raw(emergency) };
        for (signal, old) in CRASHES.iter().zip(&emergency.old) {
            unsafe { libc::sigaction(*signal, old, std::ptr::null_mut()) };
        }
    }
}

/// What the keyboard of the terminal did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A key was pressed, or with `true` it repeats.
    Down(Stroke, bool),
    Up(KeyCode, KeyMods),
}

/// The number of the Linux console that is the terminal of this program, 0 if it is none.
fn number() -> u16 {
    // The device of the terminal is the fifth number after the name of the program, which
    // is in parentheses. A console has the major number 4 and its own number as the minor.
    let status = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let device = status
        .rsplit_once(')')
        .and_then(|(_, numbers)| numbers.split_whitespace().nth(4))
        .and_then(|number| number.parse::<u32>().ok())
        .unwrap_or(0);
    let (major, minor) = ((device >> 8) & 0xfff, device & 0xff);
    if cfg!(target_os = "linux") && major == 4 && device >> 20 == 0 && (1..=63).contains(&minor) {
        minor as u16
    } else {
        0
    }
}

/// The terminal of the program as the keyboard of its window.
pub struct Console {
    guard: Guard<Terminal>,
    /// Whether what is typed on the terminal are the keys of the window. When SDL2 has the
    /// keys, the terminal only ends the program.
    typed: bool,
    /// The number of the Linux console that the terminal is, 0 if it is none.
    number: u16,
    parser: Parser,
    holds: Holds,
    _signals: Signals,
}

impl Console {
    /// Takes the terminal of the program for the keys of its window. With `screen` the window
    /// is the screen of the text console that the terminal is, and an error says why it
    /// cannot be: nothing was touched then. Without `screen`, `None` says that the program
    /// has no terminal to read keys from. Without `typed` SDL2 has the keys, and the
    /// terminal only ends the program: then it must be the console itself, because SDL2
    /// takes the keyboard of the console away from every other terminal.
    pub fn open(screen: bool, typed: bool) -> Result<Option<Self>, String> {
        let terminal = match Terminal::open() {
            Ok(terminal) => terminal,
            Err(error) if screen => {
                return Err(format!(
                    "the screen of the text console needs the active console as the terminal of \
                     the program, not a pipe or a background job: {error}"
                ))
            }
            Err(_) => return Ok(None),
        };
        let saved = match terminal.snapshot() {
            Ok(saved) if screen || saved.foreground => saved,
            Err(error) if screen => return Err(error),
            _ => return Ok(None),
        };
        if screen {
            saved.console()?;
        }
        if !typed && !terminal.virtual_console() {
            return Ok(None);
        }
        STOP.store(false, Ordering::SeqCst);
        let signals = Signals::install(if screen {
            CONSOLE_SIGNALS
        } else {
            TERMINAL_SIGNALS
        })?;
        let rate = terminal.rate();
        let fd = terminal.0.as_raw_fd();
        let guard = Guard::enter(terminal, saved, screen)?;
        arm(fd, screen, saved);
        Ok(Some(Self {
            guard,
            typed,
            number: number(),
            parser: Parser::default(),
            holds: Holds::new(rate),
            _signals: signals,
        }))
    }

    /// Whether what is typed on the terminal are the keys of the window.
    pub fn typed(&self) -> bool {
        self.typed
    }

    /// Whether the console of the program is the one that is shown. SDL2 tells the keys and
    /// the mouse of the input devices also while the user is at another console, where they
    /// are not meant for this program; a terminal has only the keys that are meant for it.
    pub fn shown(&self) -> bool {
        if self.number == 0 {
            return true;
        }
        // `VT_GETSTATE` of Linux: the console that is shown, and two numbers of no use here.
        let mut state = [0u16; 3];
        let fd = self.guard.device.0.as_raw_fd();
        unsafe { libc::ioctl(fd, 0x5603 as _, &mut state) != 0 || state[0] == self.number }
    }

    /// Starts the watcher that ends the program when it is asked to end while it is busy and
    /// stays busy. It is a thread: what must happen before the program has threads comes
    /// first.
    pub fn watch(&self) {
        watch();
    }

    /// The program has left its loop and ends by itself: nothing watches it any more.
    pub fn stand_down(&self) {
        WATCHER.store(0, Ordering::SeqCst);
    }

    /// Whether a signal or a switch to another console asks the program to end.
    pub fn stopping(&self) -> bool {
        STOP.load(Ordering::SeqCst)
    }

    /// What the keys did since the last call. The call also tells the watcher that the
    /// program is not busy.
    pub fn events(&mut self, now: Instant) -> Result<Vec<Event>, String> {
        BEAT.store(self::now(), Ordering::SeqCst);
        let mut events = Vec::new();
        for ended in self.holds.release(now) {
            let (Ended::Released(key, mods) | Ended::Tapped(key, mods)) = ended;
            events.push(Event::Up(key, mods));
            if let Ended::Tapped(..) = ended {
                // The press of a second tap that was taken for a repeat.
                let character = None;
                events.push(Event::Down(
                    Stroke {
                        key,
                        mods,
                        character,
                    },
                    false,
                ));
            }
        }
        let mut strokes = self.parser.expire(now);
        let mut bytes = [0; 256];
        for _ in 0..16 {
            match self.guard.device.0.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => strokes.extend(self.parser.feed(&bytes[..count], now)),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(format!("reading the keys of the terminal: {error}")),
            }
        }
        if !self.typed {
            // SDL2 has told the same keys already.
            return Ok(Vec::new());
        }
        for stroke in strokes {
            let repeat =
                stroke.key != KeyCode::Unknown && self.holds.press(stroke.key, stroke.mods, now);
            events.push(Event::Down(stroke, repeat));
        }
        Ok(events)
    }

    /// Gives the terminal back. Everything that draws on the screen must be gone by now.
    pub fn close(mut self) -> Result<(), String> {
        self.guard.restore()
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.stand_down();
        if let Err(error) = self.guard.restore() {
            eprintln!("error: {error}");
        }
        disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;
    use std::rc::Rc;

    #[derive(Default)]
    struct History {
        events: Vec<&'static str>,
        fail: Option<&'static str>,
        switched: bool,
    }

    struct Fake(Rc<RefCell<History>>);

    impl Fake {
        fn event(&mut self, value: &'static str) -> Result<(), String> {
            let mut history = self.0.borrow_mut();
            history.events.push(value);
            if history.fail == Some(value) {
                history.fail = None;
                return Err(format!("injected {value} failure"));
            }
            Ok(())
        }
    }

    impl Device for Fake {
        fn attributes(&mut self, value: &libc::termios) -> Result<(), String> {
            self.event(if value.c_lflag & libc::ICANON == 0 {
                "raw"
            } else {
                "cooked"
            })
        }
        fn active(&mut self) -> Result<i32, String> {
            Ok(if self.0.borrow().switched { 9 } else { 2 })
        }
        fn switching(&mut self, value: VtMode) -> Result<(), String> {
            self.event(if value.mode == VT_AUTO {
                "auto"
            } else {
                "owned"
            })
        }
        fn video(&mut self, value: i32) -> Result<(), String> {
            self.event(if value == KD_GRAPHICS {
                "graphics"
            } else {
                "text"
            })
        }
        fn keyboard(&mut self, value: i32) -> Result<(), String> {
            assert_eq!(value, K_XLATE);
            self.event("keyboard")
        }
    }

    fn saved() -> Snapshot {
        let mut attributes = unsafe { std::mem::zeroed::<libc::termios>() };
        attributes.c_lflag = libc::ICANON | libc::ECHO;
        Snapshot {
            attributes,
            foreground: true,
            index: 2,
            active: 2,
            video: KD_TEXT,
            keyboard: K_XLATE,
            switching: VtMode::default(),
        }
    }

    fn fake() -> (Fake, Rc<RefCell<History>>) {
        let log = Rc::new(RefCell::new(History::default()));
        (Fake(Rc::clone(&log)), log)
    }

    #[test]
    fn a_console_that_is_not_free_and_active_is_refused_before_anything_changes() {
        for case in 0..7 {
            let mut state = saved();
            match case {
                0 => state.active = 9,
                1 => state.index = 0,
                2 => state.foreground = false,
                3 => state.video = KD_GRAPHICS,
                4 => state.keyboard = 0,
                5 => state.switching.mode = VT_PROCESS,
                _ => (state.index, state.active) = (0, 0),
            }
            assert!(state.console().is_err(), "{case}");
            let (device, log) = fake();
            assert!(Guard::enter(device, state, true).is_err(), "{case}");
            assert!(log.borrow().events.is_empty(), "{case}");
        }
        assert!(saved().console().is_ok());
    }

    #[test]
    fn a_switch_while_the_program_starts_is_refused_before_the_screen_changes() {
        let (device, log) = fake();
        log.borrow_mut().switched = true;
        assert!(Guard::enter(device, saved(), true).is_err());
        assert_eq!(log.borrow().events, ["owned", "keyboard", "auto"]);
    }

    #[test]
    fn the_console_is_owned_before_the_screen_and_released_after_everything_else() {
        let (device, log) = fake();
        let guard = Guard::enter(device, saved(), true).unwrap();
        log.borrow_mut().events.push("the window is gone");
        drop(guard);
        assert_eq!(
            log.borrow().events,
            [
                "owned",
                "raw",
                "graphics",
                "the window is gone",
                "keyboard",
                "cooked",
                "text",
                "auto",
            ]
        );
    }

    #[test]
    fn a_terminal_that_is_only_the_keyboard_changes_its_settings_and_nothing_else() {
        let (device, log) = fake();
        // Any terminal will do: no console, and one that belongs to another program.
        let mut state = saved();
        (state.index, state.active, state.video) = (0, 0, KD_GRAPHICS);
        let mut guard = Guard::enter(device, state, false).unwrap();
        guard.restore().unwrap();
        drop(guard);
        assert_eq!(log.borrow().events, ["raw", "cooked"]);
        let (device, log) = fake();
        log.borrow_mut().fail = Some("raw");
        assert!(Guard::enter(device, saved(), false).is_err());
        assert_eq!(log.borrow().events, ["raw"]);
    }

    #[test]
    fn a_start_that_fails_gives_back_what_it_took() {
        for (failure, expected) in [
            ("owned", vec!["owned"]),
            ("raw", vec!["owned", "raw", "keyboard", "auto"]),
            (
                "graphics",
                vec!["owned", "raw", "graphics", "keyboard", "cooked", "auto"],
            ),
        ] {
            let (device, log) = fake();
            log.borrow_mut().fail = Some(failure);
            assert!(Guard::enter(device, saved(), true).is_err());
            assert_eq!(log.borrow().events, expected);
        }
    }

    #[test]
    fn a_failure_while_giving_back_does_not_skip_the_rest() {
        for failure in ["keyboard", "cooked", "text", "auto"] {
            let (device, log) = fake();
            let mut guard = Guard::enter(device, saved(), true).unwrap();
            log.borrow_mut().fail = Some(failure);
            assert!(guard.restore().unwrap_err().contains(failure));
            assert_eq!(
                &log.borrow().events[3..],
                ["keyboard", "cooked", "text", "auto"]
            );
            assert_eq!(guard.owned, failure == "auto");
            // What failed is tried once more when the guard goes.
            drop(guard);
            assert_eq!(log.borrow().events.last(), Some(&failure));
        }
    }

    #[test]
    fn a_panic_gives_the_console_back_and_giving_back_twice_does_nothing() {
        let (device, log) = fake();
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = Guard::enter(device, saved(), true).unwrap();
            panic!("the program fails while it has the screen");
        }));
        assert!(unwind.is_err());
        assert_eq!(log.borrow().events.last(), Some(&"auto"));
        let (device, log) = fake();
        let mut guard = Guard::enter(device, saved(), true).unwrap();
        guard.restore().unwrap();
        let length = log.borrow().events.len();
        guard.restore().unwrap();
        drop(guard);
        assert_eq!(log.borrow().events.len(), length);
    }

    #[cfg(target_os = "freebsd")]
    #[test]
    fn the_requests_are_those_of_the_freebsd_headers() {
        use request::*;
        assert_eq!(std::mem::size_of::<VtMode>(), 8);
        assert_eq!(std::mem::align_of::<VtMode>(), 2);
        assert_eq!(
            (KDGETMODE, KDSETMODE, KDGKBMODE, KDSKBMODE, KDGETREPEAT),
            (0x40044b09, 0x20044b0a, 0x40044b06, 0x20044b07, 0x40084b67)
        );
        assert_eq!(
            (VT_GETMODE, VT_SETMODE, VT_GETACTIVE, VT_GETINDEX),
            (0x40087603, 0x80087602, 0x40047607, 0x40047608)
        );
        assert_eq!((libc::SIGUSR1, libc::SIGUSR2), (30, 31));
    }

    /// A terminal of the test's own: what the program writes to and reads from, and the
    /// other end, where the test types.
    fn terminal() -> (Terminal, File) {
        // The name of the terminal is in memory that all threads share.
        static NAMES: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _names = NAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        unsafe {
            let typist = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(typist >= 0);
            assert_eq!(libc::grantpt(typist), 0);
            assert_eq!(libc::unlockpt(typist), 0);
            let name = std::ffi::CStr::from_ptr(libc::ptsname(typist)).to_owned();
            let terminal = libc::open(
                name.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
            );
            assert!(terminal >= 0);
            (
                Terminal(File::from_raw_fd(terminal)),
                File::from_raw_fd(typist),
            )
        }
    }

    #[test]
    fn the_keys_of_a_real_terminal_are_read_and_its_settings_come_back() {
        let (terminal, mut typist) = terminal();
        let before = terminal.snapshot().unwrap();
        // The terminal of a test is no console: its screen cannot be taken.
        assert_eq!((before.index, before.active), (0, 0));
        assert!(before.console().is_err());
        assert_ne!(before.attributes.c_lflag & libc::ICANON, 0);
        let mut console = Console {
            guard: Guard::enter(terminal, before, false).unwrap(),
            typed: true,
            number: 0,
            parser: Parser::default(),
            holds: Holds::new(None),
            _signals: Signals(Vec::new()),
        };
        let during = console.guard.device.snapshot().unwrap().attributes;
        assert_eq!(during.c_lflag & (libc::ICANON | libc::ECHO), 0);
        assert_ne!(during.c_oflag & libc::OPOST, 0);
        // Of the keys that are signals in a terminal's ordinary mode only the one that
        // interrupts the program still is: that one reaches a program that is busy.
        assert_ne!(during.c_lflag & libc::ISIG, 0);
        assert_eq!(
            during.c_cc[libc::VINTR],
            before.attributes.c_cc[libc::VINTR]
        );
        assert_eq!(
            (during.c_cc[libc::VQUIT], during.c_cc[libc::VSUSP]),
            (libc::_POSIX_VDISABLE, libc::_POSIX_VDISABLE)
        );
        let start = Instant::now();
        assert_eq!(console.events(start).unwrap(), []);
        // The interrupt key is no key of the window: the terminal makes a signal of it, for
        // the program in its foreground, which this test is not. The others are keys.
        typist.write_all(b"\x03").unwrap();
        typist.write_all(b"a\x1b[A\x1a").unwrap();
        let mut events = Vec::new();
        for attempt in 0..200 {
            events.extend(console.events(start).unwrap());
            if events.len() == 3 {
                break;
            }
            assert!(attempt < 199, "{events:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let keys: Vec<_> = events
            .iter()
            .map(|event| match event {
                Event::Down(stroke, false) => (stroke.key, stroke.mods.ctrl, stroke.character),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            keys,
            [
                (KeyCode::A, false, Some('a')),
                (KeyCode::Up, false, None),
                (KeyCode::Z, true, None)
            ]
        );
        assert!(!console.stopping());
        // The keys are released when their time is over, and the Escape key comes when its
        // character has waited for a sequence. The time is what the test says it is.
        typist.write_all(b"\x1b").unwrap();
        let later = start + Duration::from_secs(1);
        let mut events = Vec::new();
        for attempt in 0..200 {
            events.extend(
                console
                    .events(later + Duration::from_millis(60) * attempt)
                    .unwrap(),
            );
            if events.len() == 4 {
                break;
            }
            assert!(attempt < 199, "{events:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let none = KeyMods::default();
        let ctrl = KeyMods { ctrl: true, ..none };
        assert_eq!(
            events[..3],
            [
                Event::Up(KeyCode::A, none),
                Event::Up(KeyCode::Up, none),
                Event::Up(KeyCode::Z, ctrl)
            ]
        );
        assert!(
            matches!(events[3], Event::Down(stroke, false) if stroke.key == KeyCode::Escape),
            "{events:?}"
        );
        let after = {
            let terminal = Terminal(console.guard.device.0.try_clone().unwrap());
            console.close().unwrap();
            terminal.snapshot().unwrap().attributes
        };
        assert_eq!(after.c_cc, before.attributes.c_cc);
        assert_eq!(
            (after.c_iflag, after.c_oflag, after.c_cflag, after.c_lflag),
            (
                before.attributes.c_iflag,
                before.attributes.c_oflag,
                before.attributes.c_cflag,
                before.attributes.c_lflag
            )
        );
    }

    #[test]
    fn a_terminal_that_is_no_linux_console_is_always_the_one_that_is_shown() {
        let (terminal, _typist) = terminal();
        let before = terminal.snapshot().unwrap();
        let mut console = Console {
            guard: Guard::enter(terminal, before, false).unwrap(),
            typed: false,
            number: 0,
            parser: Parser::default(),
            holds: Holds::new(None),
            _signals: Signals(Vec::new()),
        };
        assert!(console.shown());
        // A console that the terminal is not: the question fails, and nothing is held back.
        console.number = 63;
        assert!(console.shown());
        // What is typed there is not told twice when SDL2 has the keys.
        assert!(!console.typed());
        console.close().unwrap();
        // The tests do not run on a Linux console, or they are not what is shown there.
        assert!(number() == 0 || cfg!(target_os = "linux"));
    }

    #[test]
    fn only_a_program_that_was_busy_when_it_was_asked_to_end_is_ended_without_its_help() {
        // Nothing asks: a program may be busy as long as it likes.
        let mut watch = Watch::Calm;
        assert!(!overdue(&mut watch, false, 0, 1_000_000));
        assert_eq!(watch, Watch::Calm);
        // Asked while it looks at its console: it ends by itself, however long that takes.
        assert!(!overdue(&mut watch, true, 5000, 5000 + BUSY - 1));
        assert_eq!(watch, Watch::Answering);
        assert!(!overdue(&mut watch, true, 5000, 5000 + 100 * PATIENCE));
        // Asked while it is busy: it has some time to look at its console again.
        let mut watch = Watch::Calm;
        let asked = 5000 + BUSY;
        assert!(!overdue(&mut watch, true, 5000, asked));
        assert_eq!(
            watch,
            Watch::Busy {
                at: asked,
                beat: 5000
            }
        );
        assert!(!overdue(&mut watch, true, 5000, asked + PATIENCE - 1));
        assert!(overdue(&mut watch, true, 5000, asked + PATIENCE));
        // It looked in time: it is trusted from then on.
        assert!(!overdue(&mut watch, true, asked + 10, asked + PATIENCE));
        assert_eq!(watch, Watch::Answering);
        assert!(!overdue(
            &mut watch,
            true,
            asked + 10,
            asked + 100 * PATIENCE
        ));
        // A request that is withdrawn starts everything again.
        assert!(!overdue(&mut watch, false, 0, asked));
        assert_eq!(watch, Watch::Calm);
    }

    #[test]
    fn a_busy_program_is_ended_with_its_terminal_given_back_and_an_answering_one_is_not() {
        const CHILD: &str = "MINIQUAD_CONSOLE_WATCH_TEST";
        let name = format!(
            "{}::a_busy_program_is_ended_with_its_terminal_given_back_and_an_answering_one_is_not",
            module_path!().split_once("::").unwrap().1
        );
        let role = std::env::var(CHILD).unwrap_or_default();
        if let Some((role, fd)) = role.split_once(':') {
            // The terminal of the test that started this process.
            let terminal = Terminal(unsafe { File::from_raw_fd(fd.parse().unwrap()) });
            let before = terminal.snapshot().unwrap();
            let fd = terminal.0.as_raw_fd();
            STOP.store(false, Ordering::SeqCst);
            let mut console = Console {
                guard: Guard::enter(terminal, before, false).unwrap(),
                typed: true,
                number: 0,
                parser: Parser::default(),
                holds: Holds::new(None),
                _signals: Signals::install(TERMINAL_SIGNALS).unwrap(),
            };
            arm(fd, false, before);
            watch();
            let pause = |milliseconds| std::thread::sleep(Duration::from_millis(milliseconds));
            if role == "busy" {
                // It calculates and does not look at its console, also not when it is asked.
                pause(BUSY + 200);
                unsafe { libc::raise(libc::SIGTERM) };
                pause(BUSY + 20 * PATIENCE);
                unreachable!("nothing ended the busy program");
            }
            // It looks at its console when it is asked, and then takes long to end.
            for _ in 0..5 {
                console.events(Instant::now()).unwrap();
                pause(20);
            }
            unsafe { libc::raise(libc::SIGTERM) };
            console.events(Instant::now()).unwrap();
            assert!(console.stopping());
            pause(BUSY + 2 * PATIENCE);
            console.stand_down();
            console.close().unwrap();
            return;
        }
        for (role, code) in [("busy", 1), ("answering", 0)] {
            let (terminal, _typist) = terminal();
            let before = terminal.snapshot().unwrap().attributes;
            let start = Instant::now();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, format!("{role}:{}", terminal.0.as_raw_fd()))
                .output()
                .unwrap();
            let (took, errors) = (start.elapsed(), String::from_utf8_lossy(&child.stderr));
            assert_eq!(child.status.code(), Some(code), "{role}: {errors}");
            assert_eq!(
                errors.contains("was busy when it was asked to end"),
                code == 1
            );
            let least = if code == 1 {
                BUSY + PATIENCE
            } else {
                BUSY + 2 * PATIENCE
            };
            assert!(took >= Duration::from_millis(least), "{role}: {took:?}");
            assert!(
                took < Duration::from_millis(BUSY + 15 * PATIENCE),
                "{role}: {took:?}"
            );
            let after = terminal.snapshot().unwrap().attributes;
            assert_eq!(
                (after.c_iflag, after.c_oflag, after.c_lflag, after.c_cc),
                (before.c_iflag, before.c_oflag, before.c_lflag, before.c_cc),
                "{role}"
            );
        }
    }

    #[cfg(target_os = "freebsd")]
    #[test]
    fn what_is_given_back_without_the_program_frees_the_screen_before_the_switching() {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::io::IntoRawFd;
        use std::os::unix::process::ExitStatusExt;

        const CHILD: &str = "MINIQUAD_CONSOLE_SCREEN_TEST";
        let name = format!(
            "{}::what_is_given_back_without_the_program_frees_the_screen_before_the_switching",
            module_path!().split_once("::").unwrap().1
        );
        if std::env::var_os(CHILD).is_none() {
            // In a process of its own: descriptors are closed that other tests may use.
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{:?} {}",
                child.status.signal(),
                String::from_utf8_lossy(&child.stderr)
            );
            return;
        }
        // The devices that stand for a screen and for something else of the program.
        let open = |path: &str| File::open(path).unwrap().into_raw_fd();
        let screen = [open("/dev/null"), open("/dev/null")];
        let others = [open("/dev/zero"), open("/etc/passwd")];
        let (terminal, _typist) = terminal();
        let mut screens = [0; SCREENS];
        screens[0] = std::fs::metadata("/dev/null").unwrap().rdev() as libc::dev_t;
        let emergency = Emergency {
            fd: terminal.0.as_raw_fd(),
            screen: true,
            saved: terminal.snapshot().unwrap(),
            screens,
            old: [unsafe { std::mem::zeroed::<libc::sigaction>() }; 5],
        };
        let is_open = |fd| unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0;
        assert!(screen.iter().chain(&others).all(|&fd| is_open(fd)));
        unsafe { give_back(&emergency) };
        assert!(screen.iter().all(|&fd| !is_open(fd)));
        assert!(others.iter().all(|&fd| is_open(fd)));
        assert!(is_open(emergency.fd));
        // The screens of this computer are found, or there are none.
        assert!(self::screens()
            .iter()
            .all(|&device| { device == 0 || std::path::Path::new("/dev/dri").exists() }));
    }

    #[test]
    fn signals_ask_the_program_to_end_and_a_crash_gives_the_terminal_back() {
        use std::os::unix::process::ExitStatusExt;

        // In processes of their own: the signals are those of a whole process.
        const CHILD: &str = "MINIQUAD_CONSOLE_SIGNAL_TEST";
        let name = format!(
            "{}::signals_ask_the_program_to_end_and_a_crash_gives_the_terminal_back",
            module_path!().split_once("::").unwrap().1
        );
        let child = |role: String| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", &name, "--nocapture"])
                .env(CHILD, role)
                .output()
                .unwrap()
        };
        let role = std::env::var(CHILD).unwrap_or_default();
        if role == "signals" {
            for &signal in CONSOLE_SIGNALS {
                let signals = Signals::install(CONSOLE_SIGNALS).unwrap();
                STOP.store(false, Ordering::SeqCst);
                assert_eq!(unsafe { libc::raise(signal) }, 0);
                assert!(STOP.load(Ordering::SeqCst), "{signal}");
                drop(signals);
            }
            // What handled a crash before handles it again when the terminal is given back.
            let current = || unsafe {
                let mut action = std::mem::zeroed::<libc::sigaction>();
                libc::sigaction(libc::SIGSEGV, std::ptr::null(), &mut action);
                action.sa_sigaction
            };
            let before = current();
            let (terminal, _typist) = terminal();
            arm(terminal.0.as_raw_fd(), false, terminal.snapshot().unwrap());
            assert_eq!(current(), crash as *const () as usize);
            disarm();
            assert_eq!(current(), before);
        } else if let Some(fd) = role.strip_prefix("crash:") {
            // The terminal of the test that started this process.
            let terminal = Terminal(unsafe { File::from_raw_fd(fd.parse().unwrap()) });
            let before = terminal.snapshot().unwrap();
            let fd = terminal.0.as_raw_fd();
            let guard = Guard::enter(terminal, before, false).unwrap();
            arm(fd, false, before);
            let during = guard.device.snapshot().unwrap().attributes;
            assert_eq!(during.c_lflag & libc::ICANON, 0);
            unsafe { libc::raise(libc::SIGSEGV) };
            unreachable!("the crash did not end the process");
        } else {
            let signals = child("signals".to_owned());
            assert!(
                signals.status.success(),
                "{}",
                String::from_utf8_lossy(&signals.stderr)
            );
            let (terminal, _typist) = terminal();
            let before = terminal.snapshot().unwrap().attributes;
            let crash = child(format!("crash:{}", terminal.0.as_raw_fd()));
            assert_eq!(
                crash.status.signal(),
                Some(libc::SIGSEGV),
                "{}",
                String::from_utf8_lossy(&crash.stderr)
            );
            let after = terminal.snapshot().unwrap().attributes;
            assert_ne!(after.c_lflag & libc::ICANON, 0);
            assert_eq!(
                (after.c_iflag, after.c_oflag, after.c_lflag, after.c_cc),
                (before.c_iflag, before.c_oflag, before.c_lflag, before.c_cc)
            );
        }
    }
}
