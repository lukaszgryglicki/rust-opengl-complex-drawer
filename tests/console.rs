//! The tests of the console back end in `third_party/miniquad`, which Linux and FreeBSD have:
//! its files contain them, and this compiles those files for them.
#![cfg(any(target_os = "linux", target_os = "freebsd"))]
#![allow(dead_code)]

mod event {
    pub use macroquad::miniquad::{KeyCode, KeyMods};
}

#[path = "../third_party/miniquad/src/native/linux_sdl2"]
mod linux_sdl2 {
    pub mod console;
    pub mod terminal;
}
