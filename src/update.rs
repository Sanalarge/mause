use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = match option_env!("MAUSE_UPDATE_REPO") {
    Some(repo) => repo,
    None => "",
};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const AFTER_UPDATE: &str = "MAUSE_UPDATED";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub asset: String,
    pub page: String,
}

fn parts(v: &str) -> (u64, u64, u64) {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|p| {
        p.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    });
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

pub fn newer(a: &str, b: &str) -> bool {
    parts(a) > parts(b)
}

fn command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn run(c: &mut Command) -> Result<(), String> {
    let out = c
        .output()
        .map_err(|e| format!("{} çalıştırılamadı: {e}", c.get_program().to_string_lossy()))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .trim()
            .lines()
            .last()
            .unwrap_or("işlem başarısız")
            .to_owned())
    }
}

fn fetch(url: &str, to: Option<&Path>) -> Result<Vec<u8>, String> {
    let curl = if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    };
    let mut c = command(curl);
    c.args(["-fsSL", "--max-time", "600", "-H", "User-Agent: mause", url]);
    if let Some(p) = to {
        c.arg("-o").arg(p);
    }
    let out = match c.output() {
        Ok(o) => o,
        Err(_) => {
            let mut w = command("wget");
            w.args(["-q", "--header=User-Agent: mause", "-O"]);
            match to {
                Some(p) => w.arg(p),
                None => w.arg("-"),
            };
            w.arg(url)
                .output()
                .map_err(|_| "indirme aracı bulunamadı (curl ya da wget gerekli)".to_owned())?
        }
    };
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "indirilemedi: {}",
            e.trim().lines().last().unwrap_or("ağ hatası")
        ));
    }
    Ok(out.stdout)
}

fn string_fields<'a>(json: &'a str, key: &str) -> Vec<&'a str> {
    let pat = format!("\"{key}\"");
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(i) = rest.find(&pat) {
        rest = &rest[i + pat.len()..];
        let Some(v) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let Some(v) = v.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(end) = v.find('"') {
            out.push(&v[..end]);
        }
    }
    out
}

fn asset_suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        "-windows-x86_64-setup.exe"
    } else if cfg!(target_os = "macos") {
        "-macos-universal.zip"
    } else if installed_by_package() {
        if Path::new("/usr/bin/dpkg").exists() {
            "-linux-x86_64.deb"
        } else {
            "-linux-x86_64.rpm"
        }
    } else {
        "-linux-x86_64.tar.gz"
    }
}

fn installed_by_package() -> bool {
    std::env::current_exe().is_ok_and(|p| p.starts_with("/usr/bin"))
}

fn parse_release(json: &str) -> Option<Release> {
    let version = string_fields(json, "tag_name")
        .first()?
        .trim_start_matches('v')
        .to_owned();
    let page = string_fields(json, "html_url")
        .first()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let assets = string_fields(json, "browser_download_url");
    let asset = assets
        .iter()
        .find(|u| u.ends_with(asset_suffix()))
        .or_else(|| {
            cfg!(target_os = "windows")
                .then(|| assets.iter().find(|u| u.ends_with("-windows-x86_64.zip")))
                .flatten()
        })?
        .to_string();
    Some(Release {
        version,
        asset,
        page,
    })
}

pub fn check() -> Result<Option<Release>, String> {
    if REPO.is_empty() {
        return Err("Güncelleme deposu bu derlemeye tanımlanmamış".to_owned());
    }
    let body = fetch(
        &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        None,
    )
    .map_err(|e| format!("Güncelleme denetlenemedi ({e}). İnternet bağlantısını denetleyin."))?;
    let r = parse_release(&String::from_utf8_lossy(&body)).ok_or("sürüm bilgisi okunamadı")?;
    Ok(newer(&r.version, VERSION).then_some(r))
}

pub fn releases_page() -> String {
    if REPO.is_empty() {
        String::new()
    } else {
        format!("https://github.com/{REPO}/releases/latest")
    }
}

pub fn install(r: &Release) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("mause-update-{}", r.version));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("geçici klasör açılamadı: {e}"))?;
    let file = dir.join(r.asset.rsplit('/').next().unwrap_or("mause-paket"));
    fetch(&r.asset, Some(&file))?;
    #[cfg(target_os = "windows")]
    if r.asset.ends_with("-setup.exe") {
        os::verify_setup(&file, &r.asset)?;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let started = os::install(&file, &dir, &exe)?;
    restart(&started);
}

fn restart(exe: &Path) -> ! {
    let mut c = Command::new(exe);
    c.args(std::env::args_os().skip(1)).env(AFTER_UPDATE, "1");
    #[cfg(target_os = "macos")]
    let c = {
        let bundle = exe.ancestors().nth(3).unwrap_or(exe).to_path_buf();
        let mut o = Command::new("open");
        o.arg("-n").arg(bundle).env(AFTER_UPDATE, "1");
        o
    };
    let _ = { c }.spawn();
    std::process::exit(0);
}

pub fn after_start() {
    if std::env::var_os(AFTER_UPDATE).is_some() {
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
        let _ = std::fs::remove_file(exe.with_extension("old.exe"));
    }
}

fn replace_exe(new: &Path, exe: &Path) -> Result<(), String> {
    let old = exe.with_extension(if cfg!(target_os = "windows") {
        "old.exe"
    } else {
        "old"
    });
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("{} değiştirilemedi: {e}", exe.display()))?;
    if let Err(e) = std::fs::copy(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("yeni sürüm yazılamadı: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod os {
    use super::*;

    pub fn install(file: &Path, dir: &Path, exe: &Path) -> Result<PathBuf, String> {
        let f = file.to_string_lossy().into_owned();
        if f.ends_with(".deb") {
            run(command("pkexec").args(["apt-get", "install", "-y", "--allow-downgrades", &f]))
                .or_else(|_| run(command("pkexec").args(["dpkg", "-i", &f])))?;
            return Ok(PathBuf::from("/usr/bin/mause"));
        }
        if f.ends_with(".rpm") {
            let script = r#"if command -v dnf >/dev/null; then dnf install -y "$1"; elif command -v zypper >/dev/null; then zypper --non-interactive install --allow-unsigned-rpm "$1"; else rpm -U --force "$1"; fi"#;
            run(command("pkexec").args(["sh", "-c", script, "sh", &f]))?;
            return Ok(PathBuf::from("/usr/bin/mause"));
        }
        run(command("tar").arg("-xzf").arg(file).arg("-C").arg(dir))?;
        let new = dir.join("mause").join("mause");
        if !new.exists() {
            return Err("paketin içinde mause bulunamadı".into());
        }
        if replace_exe(&new, exe).is_err() {
            run(command("pkexec")
                .arg("install")
                .arg("-m755")
                .arg(&new)
                .arg(exe))?;
        }
        Ok(exe.to_path_buf())
    }
}

#[cfg(target_os = "windows")]
mod os {
    use super::*;

    pub fn verify_setup(file: &Path, asset: &str) -> Result<(), String> {
        let checksum = fetch(&format!("{asset}.sha256"), None)?;
        let checksum = String::from_utf8_lossy(&checksum);
        let expected = checksum.split_whitespace().next().unwrap_or_default();
        if expected.len() != 64 || !expected.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("kurulum paketinin SHA256 özeti geçersiz".into());
        }
        let script = format!(
            "$ErrorActionPreference='Stop'; (Get-FileHash -LiteralPath '{}' -Algorithm SHA256).Hash",
            file.display().to_string().replace('\'', "''")
        );
        let output = command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|e| format!("kurulum paketi doğrulanamadı: {e}"))?;
        let actual = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || !actual.trim().eq_ignore_ascii_case(expected) {
            return Err("kurulum paketi SHA256 doğrulamasını geçemedi; yeniden indirin".into());
        }
        Ok(())
    }

    pub fn install(file: &Path, dir: &Path, exe: &Path) -> Result<PathBuf, String> {
        if file.extension().is_some_and(|ext| ext == "exe") {
            let script = format!(
                "$ErrorActionPreference='Stop'; Start-Process -FilePath '{}' -ArgumentList @('/SILENT','/SUPPRESSMSGBOXES','/NORESTART','/MAUSEUPDATE=1') -WindowStyle Normal | Out-Null",
                file.display().to_string().replace('\'', "''")
            );
            run(command("powershell.exe").args(["-NoProfile", "-Command", &script]))?;
            std::process::exit(0);
        }
        let cmd = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            file.display().to_string().replace('\'', "''"),
            dir.display().to_string().replace('\'', "''")
        );
        run(command("powershell").args(["-NoProfile", "-Command", &cmd]))?;
        let new = dir.join("mause").join("mause.exe");
        if !new.exists() {
            return Err("paketin içinde mause.exe bulunamadı".into());
        }
        replace_exe(&new, exe)?;
        Ok(exe.to_path_buf())
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::*;

    pub fn install(file: &Path, dir: &Path, exe: &Path) -> Result<PathBuf, String> {
        run(command("ditto").arg("-x").arg("-k").arg(file).arg(dir))?;
        let new = dir.join("Mause.app");
        let bundle = exe
            .ancestors()
            .nth(3)
            .filter(|b| b.extension().is_some_and(|e| e == "app"));
        let Some(bundle) = bundle else {
            replace_exe(&new.join("Contents/MacOS/mause"), exe)?;
            return Ok(exe.to_path_buf());
        };
        let old = bundle.with_extension("old.app");
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(bundle, &old)
            .map_err(|e| format!("{} değiştirilemedi: {e}", bundle.display()))?;
        if let Err(e) = run(command("ditto").arg(&new).arg(bundle)) {
            let _ = std::fs::rename(&old, bundle);
            return Err(e);
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(exe.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surumler_sayi_olarak_karsilastirilir() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("v0.2.0", "0.1.99"));
        assert!(!newer("0.1.4", "0.1.4"));
        assert!(!newer("0.1.3", "0.1.40"));
    }

    #[test]
    fn github_yanitindan_bu_pcye_uygun_paket_secilir() {
        let json = r#"{"html_url": "https://github.com/example/Mause/releases/tag/v0.1.9", "tag_name": "v0.1.9",
            "assets": [
              {"name": "a", "browser_download_url": "https://x/mause-v0.1.9-macos-universal.zip"},
              {"name": "b", "browser_download_url": "https://x/mause-v0.1.9-windows-x86_64.zip"},
              {"name": "setup", "browser_download_url": "https://x/mause-v0.1.9-windows-x86_64-setup.exe"},
              {"name": "c", "browser_download_url": "https://x/mause-v0.1.9-linux-x86_64.deb"},
              {"name": "d", "browser_download_url": "https://x/mause-v0.1.9-linux-x86_64.rpm"},
              {"name": "e", "browser_download_url": "https://x/mause-v0.1.9-linux-x86_64.tar.gz"}
            ]}"#;
        let r = parse_release(json).expect("sürüm okunmalı");
        assert_eq!(r.version, "0.1.9");
        assert!(r.page.ends_with("/v0.1.9"));
        assert!(r.asset.ends_with(asset_suffix()), "{}", r.asset);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn eski_windows_zip_surumu_de_okunur() {
        let r = parse_release(r#"{"tag_name":"v0.1.8","assets":[{"browser_download_url":"https://x/mause-v0.1.8-windows-x86_64.zip"}]}"#).unwrap();
        assert!(r.asset.ends_with("-windows-x86_64.zip"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tar_paketi_programi_degistirir() {
        let base = std::env::temp_dir().join(format!("mause-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, dir, bin) = (base.join("src/mause"), base.join("out"), base.join("bin"));
        for d in [&src, &dir, &bin] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(src.join("mause"), b"yeni").unwrap();
        let tgz = base.join("paket.tar.gz");
        run(command("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(base.join("src"))
            .arg("mause"))
        .unwrap();
        let exe = bin.join("mause");
        std::fs::write(&exe, b"eski").unwrap();
        let started = os::install(&tgz, &dir, &exe).unwrap();
        assert_eq!(started, exe);
        assert_eq!(std::fs::read(&exe).unwrap(), b"yeni");
        let _ = std::fs::remove_dir_all(&base);
    }
}
