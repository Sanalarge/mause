//! Windows virtual display lifecycle. Only the signed MttVDD adapter is changed.
use std::io;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::Devices::Display::*;
use windows_sys::Win32::Graphics::Gdi::*;

static OWNER: Mutex<()> = Mutex::new(());

fn text(w: &[u16]) -> String {
    String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
}

fn adapter() -> Option<DISPLAY_DEVICEW> {
    for i in 0..64 {
        let mut d = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if unsafe { EnumDisplayDevicesW(null(), i, &mut d, 0) } == 0 {
            break;
        }
        if text(&d.DeviceString) == "Virtual Display Driver" {
            return Some(d);
        }
    }
    None
}

pub fn available() -> bool {
    adapter().is_some()
}

pub fn park() -> Result<(), String> {
    let _owner = OWNER
        .try_lock()
        .map_err(|_| "Önce sanal ekran paylaşımını kapatın.".to_owned())?;
    let d = adapter().ok_or("Sanal ekran sürücüsü bulunamadı.")?;
    if d.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
        return Err("Ana ekran kapatılamaz. Önce fiziksel ekranı ana ekran yapın.".into());
    }
    if d.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP != 0 {
        let mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            dmFields: DM_POSITION,
            ..Default::default()
        };
        change(&d.DeviceName, &mode).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub struct Lease {
    device: [u16; 32],
    original: DEVMODEW,
    name: String,
    topology: Option<Topology>,
    _owner: MutexGuard<'static, ()>,
}

struct Topology {
    paths: Vec<DISPLAYCONFIG_PATH_INFO>,
    modes: Vec<DISPLAYCONFIG_MODE_INFO>,
}

fn topology(flags: u32) -> io::Result<Topology> {
    for _ in 0..5 {
        let (mut np, mut nm) = (0, 0);
        let code = unsafe { GetDisplayConfigBufferSizes(flags, &mut np, &mut nm) };
        if code != 0 {
            return Err(io::Error::other(format!("Ekran listesi okunamadı: {code}")));
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let code = unsafe {
            QueryDisplayConfig(
                flags,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                null_mut(),
            )
        };
        if code == 122 {
            continue;
        }
        if code != 0 {
            return Err(io::Error::other(format!("Ekran düzeni okunamadı: {code}")));
        }
        paths.truncate(np as usize);
        modes.truncate(nm as usize);
        return Ok(Topology { paths, modes });
    }
    Err(io::Error::other("Ekran düzeni değişiyor; tekrar deneyin."))
}

impl Topology {
    fn apply(&self, allow_changes: bool) -> io::Result<()> {
        let flags = SDC_APPLY
            | SDC_USE_SUPPLIED_DISPLAY_CONFIG
            | if allow_changes { SDC_ALLOW_CHANGES } else { 0 };
        let code = unsafe {
            SetDisplayConfig(
                self.paths.len() as u32,
                self.paths.as_ptr(),
                self.modes.len() as u32,
                self.modes.as_ptr(),
                flags,
            )
        };
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "Sanal ekran etkinleştirilemedi (Windows: {code})."
            )))
        }
    }
}

fn activate(device: &[u16; 32]) -> io::Result<Topology> {
    let original = topology(QDC_ONLY_ACTIVE_PATHS)?;
    let all = topology(QDC_ALL_PATHS)?;
    let mut chosen = None;
    for path in all.paths {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
        source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        source.header.adapterId = path.sourceInfo.adapterId;
        source.header.id = path.sourceInfo.id;
        let code = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) };
        if code == 0
            && text(&source.viewGdiDeviceName) == text(device)
            && path.targetInfo.targetAvailable != 0
        {
            chosen = Some(path);
            break;
        }
    }
    let mut added = chosen
        .ok_or_else(|| io::Error::other("Sanal ekran sürücüsü bağlı bir monitör sunmuyor."))?;
    added.flags = 1; // DISPLAYCONFIG_PATH_ACTIVE
    added.sourceInfo.Anonymous.modeInfoIdx = u32::MAX;
    added.targetInfo.Anonymous.modeInfoIdx = u32::MAX;
    let mut expanded = Topology {
        paths: original.paths.clone(),
        modes: original.modes.clone(),
    };
    expanded.paths.push(added);
    if let Err(e) = expanded.apply(true) {
        let _ = original.apply(false);
        return Err(e);
    }
    Ok(original)
}

fn change(device: &[u16; 32], mode: &DEVMODEW) -> io::Result<()> {
    let code = unsafe {
        ChangeDisplaySettingsExW(
            device.as_ptr(),
            mode,
            null_mut(),
            CDS_UPDATEREGISTRY,
            null(),
        )
    };
    if code == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Sanal ekran ayarlanamadı (Windows: {code})."
        )))
    }
}

impl Lease {
    pub fn begin(size: (u16, u16)) -> io::Result<Self> {
        if !matches!(
            size,
            (1280, 720) | (1920, 1080) | (2560, 1440) | (3840, 2160)
        ) {
            return Err(io::Error::other("Desteklenmeyen sanal ekran çözünürlüğü."));
        }
        let owner = OWNER
            .try_lock()
            .map_err(|_| io::Error::other("Sanal ekran başka bir paylaşımda kullanılıyor."))?;
        let d = adapter().ok_or_else(|| io::Error::other("Ekranı paylaşılan Windows PC'de Ayarlar → Ekran paylaşımı → Sanal ekran sürücüsünü kur seçeneğini kullanın."))?;
        if d.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
            return Err(io::Error::other(
                "Sanal ekran Windows'ta ana ekran olarak seçilmiş. Önce fiziksel ekranı ana ekran yapın.",
            ));
        }
        let mut original = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        if unsafe {
            EnumDisplaySettingsExW(
                d.DeviceName.as_ptr(),
                ENUM_CURRENT_SETTINGS,
                &mut original,
                0,
            )
        } == 0
        {
            original.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT;
        }
        let attached = d.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP != 0;
        if !attached {
            original.dmFields = DM_POSITION;
            original.dmPelsWidth = 0;
            original.dmPelsHeight = 0;
        }
        let saved_topology = if !attached {
            Some(activate(&d.DeviceName)?)
        } else {
            None
        };
        let lease = Self {
            device: d.DeviceName,
            original,
            name: text(&d.DeviceName).trim_start_matches(r"\\.\").to_owned(),
            topology: saved_topology,
            _owner: owner,
        };
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let mut found = false;
        for index in 0..512 {
            if unsafe { EnumDisplaySettingsExW(d.DeviceName.as_ptr(), index, &mut mode, 0) } == 0 {
                break;
            }
            if mode.dmPelsWidth == size.0 as u32
                && mode.dmPelsHeight == size.1 as u32
                && mode.dmDisplayFrequency == 60
            {
                found = true;
                break;
            }
        }
        if !found {
            return Err(io::Error::other(format!(
                "Sürücü {}×{} 60 Hz çözünürlüğünü sunmuyor. Başka bir çözünürlük seçin.",
                size.0, size.1
            )));
        }
        mode.dmFields =
            DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_BITSPERPEL | DM_DISPLAYFREQUENCY;
        mode.dmPelsWidth = size.0 as u32;
        mode.dmPelsHeight = size.1 as u32;
        mode.dmBitsPerPel = 32;
        mode.dmDisplayFrequency = 60;
        if attached {
            mode.Anonymous1.Anonymous2.dmPosition =
                unsafe { original.Anonymous1.Anonymous2.dmPosition };
        }
        if !attached {
            mode.Anonymous1.Anonymous2.dmPosition.x = crate::platform::monitors()
                .iter()
                .map(|m| m.x + m.w)
                .max()
                .unwrap_or(0);
            mode.Anonymous1.Anonymous2.dmPosition.y = 0;
        }
        change(&d.DeviceName, &mode)?;
        for _ in 0..30 {
            if lease.monitor_index().is_some() {
                return Ok(lease);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err(io::Error::other(
            "Sanal ekran etkinleşmedi; Windows görüntü ayarlarını kontrol edin.",
        ))
    }

    pub fn monitor_index(&self) -> Option<usize> {
        crate::platform::monitors()
            .iter()
            .position(|m| m.name == self.name)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let result = match &self.topology {
            Some(saved) => saved.apply(false),
            None => change(&self.device, &self.original),
        };
        if let Err(e) = result {
            eprintln!("Sanal ekran geri yüklenemedi: {e}");
        }
    }
}

pub fn setup() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let already_installed = available();
    let dir = std::env::temp_dir().join(format!(
        "mause-vdisplay-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = dir.join("Install-VirtualDisplay.ps1");
    std::fs::write(
        &script,
        include_str!("../packaging/windows/Install-VirtualDisplay.ps1"),
    )
    .map_err(|e| e.to_string())?;
    let quoted = script.to_string_lossy().replace('\'', "''");
    let command = format!(
        "$p = Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList '-NoProfile -ExecutionPolicy Bypass -File \"{quoted}\"'; exit $p.ExitCode"
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &command])
        .creation_flags(0x08000000)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        if !already_installed {
            for _ in 0..100 {
                if available() {
                    return park();
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            return Err("Sürücü kuruldu; Windows'u yeniden başlatıp tekrar deneyin.".into());
        }
        Ok(())
    } else {
        Err(std::fs::read_to_string(dir.join("result.txt"))
            .unwrap_or_else(|_| "Sürücü kurulumu tamamlanmadı; yönetici izni gerekli.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_size_without_changing_displays() {
        assert!(Lease::begin((1, 1)).is_err());
        assert!(Lease::begin((65535, 65535)).is_err());
    }

    #[test]
    #[ignore = "captures real screens and changes display layout"]
    fn live_stream_switches_primary_virtual_and_primary() {
        use std::io::Write;
        use std::sync::atomic::{AtomicBool, Ordering};
        let before = crate::platform::monitors();
        let stop = AtomicBool::new(false);
        let opts = Mutex::new(crate::screen::Opts {
            h264: false,
            max_w: 0,
            fps: 30,
            ..Default::default()
        });
        struct Writer<'a> {
            stop: &'a AtomicBool,
            opts: &'a Mutex<crate::screen::Opts>,
            sizes: Vec<(u16, u16)>,
        }
        impl Write for Writer<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.sizes.push((
                    u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
                    u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
                ));
                match self.sizes.len() {
                    1 => self.opts.lock().unwrap().extend = Some((1920, 1080)),
                    2 => self.opts.lock().unwrap().extend = None,
                    _ => self.stop.store(true, Ordering::Relaxed),
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = Writer {
            stop: &stop,
            opts: &opts,
            sizes: Vec::new(),
        };
        crate::screen::send(&mut writer, &stop, &opts, &Mutex::new(None)).unwrap();
        let primary = (before[0].w as u16, before[0].h as u16);
        assert_eq!(writer.sizes, vec![primary, (1920, 1080), primary]);
        assert_eq!(crate::platform::monitors(), before);
    }

    #[test]
    #[ignore = "changes Windows display layout; requires installed signed driver"]
    fn virtual_monitor_can_be_captured_and_restores_layout() {
        let before = crate::platform::monitors();
        println!("Before: {before:?}");
        let display = Lease::begin((1920, 1080)).expect("virtual monitor activation");
        let index = display
            .monitor_index()
            .expect("virtual monitor enumeration");
        let (frame, monitor) =
            crate::platform::capture_monitor(index).expect("virtual monitor capture");
        assert_eq!((monitor.w, monitor.h), (1920, 1080));
        assert_eq!((frame.w, frame.h), (1920, 1080));
        assert!(
            Lease::begin((1920, 1080)).is_err(),
            "concurrent lease must not alter the display"
        );
        drop(display);
        assert_eq!(
            crate::platform::monitors(),
            before,
            "original display layout must be restored"
        );
    }

    #[test]
    #[ignore = "changes Windows display layout; requires installed signed driver"]
    fn detached_virtual_monitor_is_activated_and_detached_on_drop() {
        struct Restore(Topology);
        impl Drop for Restore {
            fn drop(&mut self) {
                self.0.apply(false).expect("restore test layout");
            }
        }
        let _restore = Restore(topology(QDC_ONLY_ACTIVE_PATHS).unwrap());
        let d = adapter().expect("installed adapter");
        assert_eq!(d.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE, 0);
        let detached = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            dmFields: DM_POSITION,
            ..Default::default()
        };
        change(&d.DeviceName, &detached).expect("detach virtual display");
        let before = crate::platform::monitors();
        let display = Lease::begin((1920, 1080)).expect("activate detached display");
        let index = display.monitor_index().unwrap();
        assert_eq!(crate::platform::capture_monitor(index).unwrap().0.w, 1920);
        drop(display);
        assert_eq!(crate::platform::monitors(), before);
    }
}
