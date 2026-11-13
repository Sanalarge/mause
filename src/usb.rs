use crate::net;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

// 24807 is reserved for the camera stream.
pub const PORT: u16 = 24808;
pub const LEGACY_PORT: u16 = 24807;
const USBIP_PORT: u16 = 3240;

pub const LIST_REQ: u8 = 0;
pub const LIST: u8 = 1;
pub const ATTACH: u8 = 2;
pub const READY: u8 = 3;
pub const FAIL: u8 = 4;
pub const DETACH: u8 = 5;
pub const ATTACHED: u8 = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct Dev {
    pub busid: String,
    pub id: String,
    pub name: String,
}

pub fn encode_list(devs: &[Dev]) -> String {
    devs.iter()
        .map(|d| {
            format!(
                "{}\t{}\t{}",
                d.busid,
                d.id,
                d.name.replace(['\t', '\n'], " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn decode_list(s: &str) -> Vec<Dev> {
    s.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            Some(Dev {
                busid: it.next()?.to_owned(),
                id: it.next()?.to_owned(),
                name: it.next().unwrap_or("").to_owned(),
            })
        })
        .filter(|d| valid_busid(&d.busid))
        .collect()
}

pub fn valid_busid(b: &str) -> bool {
    let Some((bus, device)) = b.split_once('-') else {
        return false;
    };
    b.len() <= 32
        && !bus.is_empty()
        && !bus.starts_with('0')
        && bus.bytes().all(|c| c.is_ascii_digit())
        && device
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| {
        format!(
            "{} çalıştırılamadı: {e}",
            cmd.get_program().to_string_lossy()
        )
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(text
            .trim()
            .lines()
            .last()
            .unwrap_or("işlem başarısız")
            .to_owned())
    }
}

#[cfg(target_os = "linux")]
mod os {
    use super::*;

    pub const HELP: &str = "USB paylaşımı için USB/IP araçları gerekli. İlk araç kurulumu yönetici onayı ister; paylaşım işlemlerinde tekrar parola sorulmaz.";

    pub fn install_tools() -> Result<(), String> {
        let script = r#"if command -v apt-get >/dev/null; then
  apt-get install -y usbip || apt-get install -y linux-tools-common "linux-tools-$(uname -r)" || apt-get install -y linux-tools-generic
elif command -v dnf >/dev/null; then dnf install -y usbip
elif command -v zypper >/dev/null; then zypper --non-interactive install usbip
elif command -v pacman >/dev/null; then pacman -S --noconfirm usbip
else exit 1; fi"#;
        run(Command::new("pkexec").args(["sh", "-c", script])).map(drop)
    }

    pub fn tools() -> Result<(), String> {
        let found = ["/usr/sbin/usbip", "/usr/bin/usbip", "/sbin/usbip"]
            .iter()
            .any(|p| std::path::Path::new(p).exists())
            || Command::new("usbip").arg("version").output().is_ok();
        if found { Ok(()) } else { Err(HELP.into()) }
    }

    pub fn list_local() -> Result<Vec<Dev>, String> {
        let rd = std::fs::read_dir("/sys/bus/usb/devices")
            .map_err(|e| format!("USB cihazları okunamadı: {e}"))?;
        let mut v = Vec::new();
        for e in rd.flatten() {
            let busid = e.file_name().to_string_lossy().into_owned();
            if !valid_busid(&busid) {
                continue;
            }
            let p = e.path();
            let read = |f: &str| {
                std::fs::read_to_string(p.join(f))
                    .map(|s| s.trim().to_owned())
                    .unwrap_or_default()
            };
            if read("bDeviceClass") == "09" {
                continue;
            }
            let id = format!("{}:{}", read("idVendor"), read("idProduct"));
            let name = [read("manufacturer"), read("product")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            v.push(Dev {
                busid,
                name: if name.is_empty() {
                    format!("USB cihazı {id}")
                } else {
                    name
                },
                id,
            });
        }
        v.sort_by(|a, b| a.busid.cmp(&b.busid));
        Ok(v)
    }

    const HELPER: &str = "/usr/libexec/mause-usb-helper";
    const VERSION: &str = "1";

    pub fn access_ready() -> bool {
        use std::os::unix::fs::MetadataExt;
        let trusted = |path: &str| {
            std::fs::metadata(path)
                .is_ok_and(|m| m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0)
        };
        trusted(HELPER)
            && trusted("/usr/share/mause/usb-access.version")
            && trusted("/usr/share/polkit-1/actions/org.mause.usb.policy")
            && std::fs::read_to_string("/usr/share/mause/usb-access.version")
                .is_ok_and(|s| s.trim() == VERSION)
    }

    pub fn configure_access() -> Result<(), String> {
        use std::os::unix::fs::DirBuilderExt;
        let mut random = [0; 8];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let dir = std::env::temp_dir().join(format!(
            "mause-usb-setup-{:016x}",
            u64::from_le_bytes(random)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| e.to_string())?;
        let result = (|| {
            for (name, text) in [
                (
                    "mause-usb-helper",
                    include_str!("../packaging/linux/usb/mause-usb-helper"),
                ),
                (
                    "org.mause.usb.policy",
                    include_str!("../packaging/linux/usb/org.mause.usb.policy"),
                ),
                (
                    "50-mause-usb.rules",
                    include_str!("../packaging/linux/usb/50-mause-usb.rules"),
                ),
                (
                    "install-access.sh",
                    include_str!("../packaging/linux/usb/install-access.sh"),
                ),
                ("usb-access.version", "1\n"),
            ] {
                std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
            }
            run(Command::new("pkexec")
                .arg("/bin/sh")
                .arg(dir.join("install-access.sh"))
                .arg(&dir))
            .map(drop)
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    fn privileged(operation: &str, busid: &str, port: Option<u16>) -> Result<(), String> {
        if !valid_busid(busid) {
            return Err("Geçersiz USB cihaz adresi".into());
        }
        if !access_ready() {
            return Err("USB erişimi henüz ayarlanmadı. USB sayfasındaki ‘Parolasız USB erişimini ayarla’ düğmesini bir kez kullanın.".into());
        }
        let mut command = Command::new("pkexec");
        command
            .arg("--disable-internal-agent")
            .arg(HELPER)
            .arg(operation)
            .arg(busid);
        if let Some(port) = port {
            command.arg(port.to_string());
        }
        run(&mut command).map(drop)
    }

    pub fn bind(busid: &str) -> Result<(), String> {
        privileged("bind", busid, None)
    }
    pub fn unbind(busid: &str) -> Result<(), String> {
        privileged("unbind", busid, None)
    }
    pub fn attach(local: SocketAddr, busid: &str) -> Result<(), String> {
        if !local.ip().is_loopback() || local.port() == 0 {
            return Err("USB yalnız yerel şifreli tünel üzerinden bağlanabilir".into());
        }
        privileged("attach", busid, Some(local.port()))
    }
    pub fn detach(busid: &str) -> Result<(), String> {
        privileged("detach", busid, None)
    }

    pub fn local_addr() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }
}

#[cfg(target_os = "windows")]
mod os {
    use super::*;
    use std::os::windows::process::CommandExt;

    pub const HELP: &str = "USB paylaşımı için iki küçük sürücü gerekli (usbipd-win ve usbip-win2). \"Araçları kur\"a basın.";

    pub fn install_tools() -> Result<(), String> {
        let r = run(cmd("winget").args([
            "install",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "dorssel.usbipd-win",
        ]));
        crate::platform::open_path(std::path::Path::new(
            "https://github.com/vadimgrn/usbip-win2/releases/latest",
        ));
        r.map(drop)
            .map_err(|e| format!("usbipd-win kurulamadı: {e}"))
    }
    const NO_WINDOW: u32 = 0x0800_0000;

    fn cmd(program: &str) -> Command {
        let program = if program == "usbipd" {
            installed_tool("usbipd-win", "usbipd.exe")
        } else {
            program.into()
        };
        let mut c = Command::new(program);
        c.creation_flags(NO_WINDOW);
        c
    }

    fn usbip() -> String {
        installed_tool("USBip", "usbip.exe")
    }

    fn installed_tool(directory: &str, executable: &str) -> String {
        let root = std::env::var_os("ProgramW6432")
            .or_else(|| std::env::var_os("ProgramFiles"))
            .unwrap_or_else(|| r"C:\Program Files".into());
        let installed = std::path::PathBuf::from(root)
            .join(directory)
            .join(executable);
        if installed.is_file() {
            installed.to_string_lossy().into_owned()
        } else {
            executable.into()
        }
    }

    pub fn tools() -> Result<(), String> {
        let export = cmd("usbipd").arg("--version").output().is_ok();
        let import = cmd(&usbip()).arg("--version").output().is_ok();
        if export || import {
            Ok(())
        } else {
            Err(HELP.into())
        }
    }

    pub fn list_local() -> Result<Vec<Dev>, String> {
        let text =
            run(cmd("usbipd").arg("list")).map_err(|e| format!("usbipd-win gerekli ({e})"))?;
        let mut v = Vec::new();
        let mut on = false;
        for line in text.lines() {
            if line.starts_with("Connected") {
                on = true;
                continue;
            }
            if on && line.trim().is_empty() {
                break;
            }
            let mut it = line.split_whitespace();
            let (Some(busid), Some(id)) = (it.next(), it.next()) else {
                continue;
            };
            if !on || !valid_busid(busid) || !id.contains(':') {
                continue;
            }
            let rest: Vec<&str> = it.collect();
            let name = rest.join(" ");
            let name = ["Not shared", "Shared (forced)", "Shared", "Attached"]
                .iter()
                .fold(name, |n, s| n.trim_end_matches(s).trim().to_owned());
            v.push(Dev {
                busid: busid.into(),
                id: id.into(),
                name,
            });
        }
        Ok(v)
    }

    fn elevated(args: &str) -> Result<(), String> {
        let ps = format!(
            "$p = Start-Process -FilePath '{}' -ArgumentList '{args}' -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode",
            installed_tool("usbipd-win", "usbipd.exe").replace('\'', "''")
        );
        run(cmd("powershell").args(["-NoProfile", "-Command", &ps])).map(drop)
    }

    pub fn bind(busid: &str) -> Result<(), String> {
        elevated(&format!("bind --busid {busid}"))
    }

    pub fn unbind(busid: &str) -> Result<(), String> {
        elevated(&format!("unbind --busid {busid}"))
    }

    pub fn attach(local: SocketAddr, busid: &str) -> Result<(), String> {
        let args = super::windows_attach_args(local, busid)?;
        run(cmd(&usbip()).args(args)).map(drop)
    }

    pub fn detach(busid: &str) -> Result<(), String> {
        let ports = run(cmd(&usbip()).arg("port"))?;
        let port = super::port_of(&ports, busid).ok_or("cihaz bu bilgisayara bağlı görünmüyor")?;
        run(cmd(&usbip()).args(["detach", "-p", &port])).map(drop)
    }

    pub fn local_addr() -> SocketAddr {
        // usbipd binds 3240 on all addresses, including loopback.
        SocketAddr::from(([127, 0, 0, 1], 0))
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::*;

    pub const HELP: &str = "macOS'ta USB/IP sürücüsü olmadığı için USB paylaşımı kullanılamaz.";

    pub fn tools() -> Result<(), String> {
        Err(HELP.into())
    }
    pub fn install_tools() -> Result<(), String> {
        Err(HELP.into())
    }
    pub fn list_local() -> Result<Vec<Dev>, String> {
        Err(HELP.into())
    }
    pub fn bind(_: &str) -> Result<(), String> {
        Err(HELP.into())
    }
    pub fn unbind(_: &str) -> Result<(), String> {
        Err(HELP.into())
    }
    pub fn attach(_: SocketAddr, _: &str) -> Result<(), String> {
        Err(HELP.into())
    }
    pub fn detach(_: &str) -> Result<(), String> {
        Err(HELP.into())
    }
    pub fn local_addr() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }
}

#[cfg(target_os = "linux")]
pub use os::{access_ready, configure_access};
pub use os::{attach, bind, detach, install_tools, list_local, tools, unbind};

#[cfg(any(target_os = "windows", test))]
fn windows_attach_args(local: SocketAddr, busid: &str) -> Result<Vec<String>, String> {
    if !local.ip().is_loopback() || local.port() < 1024 || !valid_busid(busid) {
        return Err("USB yalnız yerel şifreli tünel üzerinden bağlanabilir.".into());
    }
    Ok(vec![
        "--tcp-port".into(),
        local.port().to_string(),
        "attach".into(),
        "--remote".into(),
        local.ip().to_string(),
        "--bus-id".into(),
        busid.into(),
        "--once".into(),
    ])
}

fn port_of(text: &str, busid: &str) -> Option<String> {
    let mut port = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Port ") {
            port = rest.split(':').next().map(|p| p.trim().to_owned());
        } else if l.contains("usbip://") && l.ends_with(&format!("/{busid}")) {
            return port;
        }
    }
    None
}

pub struct Tunnel {
    stop: Arc<AtomicBool>,
    pub local: SocketAddr,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn pipe(tcp: TcpStream, sec: net::Secure) {
    let (Ok(mut tcp_r), Ok(mut sec_r)) = (tcp.try_clone(), sec.try_clone()) else {
        return;
    };
    let (mut tcp_w, mut sec_w) = (tcp, sec);
    let up = thread::spawn(move || {
        let _ = io::copy(&mut tcp_r, &mut sec_w);
        sec_w.shutdown();
    });
    let _ = io::copy(&mut sec_r, &mut tcp_w);
    let _ = tcp_w.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
}

fn listen(
    l: TcpListener,
    stop: Arc<AtomicBool>,
    on_conn: impl Fn(TcpStream) + Send + 'static,
) -> io::Result<SocketAddr> {
    l.set_nonblocking(true)?;
    let local = l.local_addr()?;
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((s, _)) => {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_nodelay(true);
                    on_conn(s);
                }
                Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(local)
}

pub fn serve(peer_ip: String, secret: String, port: u16) -> io::Result<Tunnel> {
    serve_on(
        TcpListener::bind(("0.0.0.0", port))?,
        peer_ip,
        secret,
        SocketAddr::from(([127, 0, 0, 1], USBIP_PORT)),
    )
}

fn serve_on(
    l: TcpListener,
    peer_ip: String,
    secret: String,
    upstream: SocketAddr,
) -> io::Result<Tunnel> {
    let stop = Arc::new(AtomicBool::new(false));
    let local = listen(l, stop.clone(), move |s| {
        if s.peer_addr().map(|a| a.ip().to_string()).ok().as_ref() != Some(&peer_ip) {
            return;
        }
        let secret = secret.clone();
        thread::spawn(move || {
            let Ok(sec) = net::handshake(s, false, &secret) else {
                return;
            };
            match TcpStream::connect(upstream) {
                Ok(t) => {
                    let _ = t.set_nodelay(true);
                    pipe(t, sec);
                }
                Err(e) => eprintln!("USB/IP sunucusuna bağlanılamadı: {e}"),
            }
        });
    })?;
    Ok(Tunnel { stop, local })
}

pub fn connect(peer_ip: String, secret: String, port: u16) -> io::Result<Tunnel> {
    let peer = format!("{peer_ip}:{port}")
        .parse()
        .map_err(io::Error::other)?;
    connect_on(TcpListener::bind(os::local_addr())?, peer, secret)
}

fn connect_on(l: TcpListener, peer: SocketAddr, secret: String) -> io::Result<Tunnel> {
    let stop = Arc::new(AtomicBool::new(false));
    let local = listen(l, stop.clone(), move |s| {
        let secret = secret.clone();
        thread::spawn(move || {
            let r = (|| -> io::Result<net::Secure> {
                let t = TcpStream::connect_timeout(&peer, Duration::from_secs(3))?;
                t.set_nodelay(true)?;
                net::handshake(t, true, &secret)
            })();
            match r {
                Ok(sec) => pipe(s, sec),
                Err(e) => eprintln!("USB tüneli kurulamadı: {e}"),
            }
        });
    })?;
    Ok(Tunnel { stop, local })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn windows_import_uses_ephemeral_tunnel_port_and_no_background_retry() {
        let local: SocketAddr = "127.0.0.1:45001".parse().unwrap();
        let args = windows_attach_args(local, "2-1").unwrap();
        assert_eq!(&args[..3], &["--tcp-port", "45001", "attach"]);
        assert!(args.iter().any(|a| a == "--once"));
        assert!(windows_attach_args("192.0.2.1:45001".parse().unwrap(), "2-1").is_err());
        assert!(windows_attach_args(local, "2-1;bad").is_err());
        assert_ne!(PORT, crate::hardware::CAMERA_PORT);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires running usbipd and installed USBip client; lists devices only"]
    fn installed_windows_client_lists_devices_through_encrypted_tunnel() {
        let server = serve_on(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            "127.0.0.1".into(),
            "test-usb-secret".into(),
            "127.0.0.1:3240".parse().unwrap(),
        )
        .unwrap();
        let client = connect_on(
            TcpListener::bind(os::local_addr()).unwrap(),
            server.local,
            "test-usb-secret".into(),
        )
        .unwrap();
        assert_ne!(client.local.port(), USBIP_PORT);
        let exe = std::path::Path::new(r"C:\Program Files\USBip\usbip.exe");
        let result = Command::new(exe)
            .args([
                "--tcp-port",
                &client.local.port().to_string(),
                "list",
                "-r",
                "127.0.0.1",
            ])
            .output()
            .unwrap();
        println!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.status.success());
    }

    #[test]
    fn liste_gidip_gelir_ve_tehlikeli_busid_elenir() {
        let devs = vec![
            Dev {
                busid: "1-1.2".into(),
                id: "046d:c52b".into(),
                name: "Logitech\tUnifying".into(),
            },
            Dev {
                busid: "3-4".into(),
                id: "0781:5581".into(),
                name: "SanDisk Ultra".into(),
            },
        ];
        let back = decode_list(&encode_list(&devs));
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "Logitech Unifying");
        assert_eq!(back[1], devs[1]);
        assert!(decode_list("1-1; rm -rf /\t1:2\tkötü").is_empty());
        assert!(!valid_busid("$(reboot)") && !valid_busid("") && valid_busid("2-1.4.3"));
    }

    #[test]
    fn usbip_port_ciktisindan_port_bulunur() {
        let text = "Imported USB devices\n====================\nPort 00: <Port in Use> at High Speed(480Mbps)\n       SanDisk : Ultra (0781:5581)\n       1-1 -> usbip://127.0.0.1:45001/3-4\n           -> remote bus/dev 003/004\nPort 08: <Port in Use> at Super Speed(5000Mbps)\n       2-1 -> usbip://127.0.0.1:45001/1-1.2\n";
        assert_eq!(port_of(text, "3-4").as_deref(), Some("00"));
        assert_eq!(port_of(text, "1-1.2").as_deref(), Some("08"));
        assert_eq!(port_of(text, "1-1"), None);
    }

    #[test]
    fn tunel_iki_yone_tasir() {
        let fake = TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream = fake.local_addr().unwrap();
        let echo = thread::spawn(move || {
            let (mut s, _) = fake.accept().unwrap();
            let mut b = [0u8; 5];
            s.read_exact(&mut b).unwrap();
            s.write_all(&b.map(|x| x.to_ascii_uppercase())).unwrap();
        });
        let server = serve_on(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            "127.0.0.1".into(),
            "ortak-parola".into(),
            upstream,
        )
        .unwrap();
        let cli = connect_on(
            TcpListener::bind("127.0.0.1:0").unwrap(),
            server.local,
            "ortak-parola".into(),
        )
        .expect("tünel ucu");
        let mut s = TcpStream::connect(cli.local).unwrap();
        s.write_all(b"merha").unwrap();
        let mut b = [0u8; 5];
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"MERHA");
        echo.join().unwrap();
    }
}
