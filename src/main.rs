#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod audio;
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[path = "audio_os.rs"]
mod audio;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[path = "audio_stub.rs"]
mod audio;
mod chrome;
#[cfg(target_os = "linux")]
mod clip;
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[path = "clip_os.rs"]
mod clip;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[path = "clip_stub.rs"]
mod clip;
mod clipboard_sync;
mod config;
mod core;
mod design;
mod discover;
mod flows;
mod gui;
mod hardware;
mod keymap;
mod logo;
mod net;
mod platform;
mod pointer;
mod screen;
mod splash;
#[cfg(test)]
mod test_support;
mod transfer;
mod tray;
mod update;
mod usb;
mod vcam;
mod video;

use config::{Config, Side};

fn main() {
    platform::init();
    update::after_start();
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(target_os = "windows")]
    if !args.is_empty() {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    if args.is_empty() {
        gui::run();
        return;
    }
    if args[0] == "devices" {
        #[cfg(target_os = "linux")]
        for l in platform::capture_report() {
            println!("{l}");
        }
        return;
    }
    let mut c = Config::load();
    discover::set_self_id(&c.id);
    if !matches!(args[0].as_str(), "run" | "host" | "peer") {
        usage();
    }
    let mut it = args.iter().skip(1);
    while let Some(k) = it.next() {
        let v = it.next().cloned().unwrap_or_else(|| usage());
        match k.as_str() {
            "--peer" => match v.rsplit_once(':') {
                Some((a, p)) => (c.addr, c.port) = (a.into(), p.into()),
                None => c.addr = v,
            },
            "--listen" => c.port = v.rsplit(':').next().unwrap_or("24800").into(),
            "--secret" => c.secret = v,
            "--side" => c.side = Side::from_key(&v),
            "--size" => {
                let (w, h) = v.split_once('x').unwrap_or_else(|| usage());
                c.w = w.parse().unwrap_or_else(|_| usage());
                c.h = h.parse().unwrap_or_else(|_| usage());
            }
            _ => usage(),
        }
    }
    if !args.iter().any(|a| a == "--size") {
        if let Some((w, h)) = platform::detect_screen() {
            (c.w, c.h) = (w, h);
        }
    }
    if c.w <= 0 || c.h <= 0 {
        (c.w, c.h) = (1920, 1080);
    }
    if !config::secret_ok(&c.secret) {
        eprintln!("hata: parola en az 4 karakter olmalı.");
        std::process::exit(2);
    }
    let sh = core::Shared::new(true);
    core::run(c, sh);
}

fn usage() -> ! {
    eprintln!(
        "kullanım: mause   (arayüz)\n          mause run [--peer IP[:PORT]] [--listen PORT] --secret PAROLA [--side left|right|top|bottom] [--size WxH]"
    );
    std::process::exit(2)
}
