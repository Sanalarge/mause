use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const PORT: u16 = 24804;

const MAGIC: &[u8; 4] = b"MFT2";
const PUT: &[u8; 4] = b"MFP1";
const LIST: &[u8; 4] = b"MFL1";
const GET: &[u8; 4] = b"MFG1";
const MKDIR: &[u8; 4] = b"MFK1";
const DELETE: &[u8; 4] = b"MFX1";
const MAX_ENTRIES: usize = 5000;
const DISKS: &str = "Diskler";
const CHUNK: usize = 256 * 1024;
const NO_ACCESS: &str =
    "Bu cihaz dosya erişimine izin vermiyor (telefonda Mause'u açıp Dosya erişimini etkinleştirin).";

pub struct Roots {
    pub share: PathBuf,
    pub inbox: PathBuf,
}

struct Entry {
    name: String,
    dir: bool,
    size: u64,
    modified: i64,
}

fn put_str(w: &mut impl Write, text: &str) -> io::Result<()> {
    let bytes = &text.as_bytes()[..text.len().min(u16::MAX as usize)];
    w.write_all(&(bytes.len() as u16).to_le_bytes())?;
    w.write_all(bytes)
}

fn get_str(r: &mut impl Read) -> io::Result<String> {
    let mut length = [0u8; 2];
    r.read_exact(&mut length)?;
    let mut bytes = vec![0u8; u16::from_le_bytes(length) as usize];
    r.read_exact(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_error(w: &mut impl Write, message: &str) -> io::Result<()> {
    w.write_all(&[0])?;
    put_str(w, message)
}

fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("dosya");
    let base: String = base.chars().filter(|c| !c.is_control()).collect();
    let base = base.trim().trim_start_matches('.');
    if base.is_empty() {
        "dosya".into()
    } else {
        base.chars().take(200).collect()
    }
}

fn unique(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_owned(), format!(".{extension}")),
        _ => (name.to_owned(), String::new()),
    };
    (1..)
        .map(|index| dir.join(format!("{stem} ({index}){extension}")))
        .find(|candidate| !candidate.exists())
        .unwrap()
}

fn locate(roots: &Roots, base: &str, child: &str) -> io::Result<(PathBuf, PathBuf)> {
    let root = fs::canonicalize(&roots.share)?;
    let base = if base.is_empty() {
        root.clone()
    } else {
        PathBuf::from(base)
    };
    let joined = if child.is_empty() {
        base
    } else {
        base.join(child)
    };
    Ok((root, fs::canonicalize(joined)?))
}

fn resolve(roots: &Roots, base: &str, child: &str) -> io::Result<PathBuf> {
    let (root, path) = locate(roots, base, child)?;
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "bu konuma erişilemez",
        ))
    }
}

fn listing(roots: &Roots, base: &str, child: &str) -> io::Result<(String, Vec<Entry>)> {
    if base == DISKS && (child.is_empty() || child == "..") {
        let disk = Entry {
            name: roots.share.to_string_lossy().into_owned(),
            dir: true,
            size: 0,
            modified: 0,
        };
        return Ok((DISKS.to_owned(), vec![disk]));
    }
    let (root, path) = locate(roots, base, child)?;
    let path = if path.starts_with(&root) { path } else { root };
    Ok((path.to_string_lossy().into_owned(), list_dir(&path)?))
}

fn list_dir(path: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = fs::read_dir(path)?
        .filter_map(Result::ok)
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = fs::metadata(item.path()).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |elapsed| elapsed.as_secs() as i64);
            Some(Entry {
                name,
                dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified,
            })
        })
        .take(MAX_ENTRIES)
        .collect();
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

pub fn serve_connection<S: Read + Write>(
    stream: &mut S,
    roots: &Roots,
    allowed: bool,
    notify: &dyn Fn(&str),
) -> io::Result<()> {
    let mut magic = [0u8; 4];
    stream.read_exact(&mut magic)?;
    if !allowed {
        if [LIST, GET, PUT, MKDIR, DELETE].contains(&&magic) {
            get_str(stream)?;
            get_str(stream)?;
            write_error(stream, NO_ACCESS)?;
        }
        return Ok(());
    }
    match &magic {
        MAGIC => receive(stream, roots, false, notify),
        PUT => receive(stream, roots, true, notify),
        LIST => serve_list(stream, roots),
        GET => serve_get(stream, roots, notify),
        MKDIR => serve_change(stream, roots, true, notify),
        DELETE => serve_change(stream, roots, false, notify),
        _ => Ok(()),
    }
}

fn serve_change(
    stream: &mut (impl Read + Write),
    roots: &Roots,
    create: bool,
    notify: &dyn Fn(&str),
) -> io::Result<()> {
    let (dir, name) = (get_str(stream)?, get_str(stream)?);
    let name = safe_name(&name);
    let path = match resolve(roots, &dir, "") {
        Ok(parent) => parent.join(&name),
        Err(error) => return write_error(stream, &format!("klasör açılamadı: {error}")),
    };
    let result = if create {
        if fs::symlink_metadata(&path).is_ok() {
            Err(io::Error::new(io::ErrorKind::AlreadyExists, "bu adla bir öğe zaten var"))
        } else {
            fs::create_dir(&path)
        }
    } else {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(&path),
            Ok(_) => fs::remove_file(&path),
            Err(error) => Err(error),
        }
    };
    match result {
        Ok(()) => {
            notify(&if create {
                format!("Bilgisayar klasör oluşturdu: {name}")
            } else {
                format!("Bilgisayar sildi: {name}")
            });
            stream.write_all(&[1])
        }
        Err(error) => write_error(stream, &error.to_string()),
    }
}

fn serve_list(stream: &mut (impl Read + Write), roots: &Roots) -> io::Result<()> {
    let (base, child) = (get_str(stream)?, get_str(stream)?);
    let out = encode_listing(roots, &base, &child)?;
    stream.write_all(&out)
}

fn encode_listing(roots: &Roots, base: &str, child: &str) -> io::Result<Vec<u8>> {
    let (path, entries) = match listing(roots, base, child) {
        Ok(found) => found,
        Err(error) => {
            let mut out = Vec::new();
            write_error(&mut out, &format!("klasör açılamadı: {error}"))?;
            return Ok(out);
        }
    };
    let mut out = vec![1u8];
    put_str(&mut out, &path)?;
    out.extend((entries.len() as u32).to_le_bytes());
    for entry in &entries {
        put_str(&mut out, &entry.name)?;
        out.push(entry.dir as u8);
        out.extend(entry.size.to_le_bytes());
        out.extend(entry.modified.to_le_bytes());
    }
    Ok(out)
}

fn serve_get(
    stream: &mut (impl Read + Write),
    roots: &Roots,
    notify: &dyn Fn(&str),
) -> io::Result<()> {
    let (dir, name) = (get_str(stream)?, get_str(stream)?);
    let path = match resolve(roots, &dir, &safe_name(&name)) {
        Ok(path) if path.is_file() => path,
        Ok(_) => return write_error(stream, "bu bir dosya değil"),
        Err(error) => return write_error(stream, &format!("dosya açılamadı: {error}")),
    };
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(error) => return write_error(stream, &format!("dosya açılamadı: {error}")),
    };
    let total = file.metadata()?.len();
    stream.write_all(&[1])?;
    stream.write_all(&total.to_le_bytes())?;
    let mut buffer = vec![0u8; CHUNK];
    let mut left = total;
    while left > 0 {
        let want = (left as usize).min(buffer.len());
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(io::Error::other("dosya okunurken kısaldı"));
        }
        stream.write_all(&buffer[..read])?;
        left -= read as u64;
    }
    notify(&format!("Bilgisayar indirdi: {name}"));
    Ok(())
}

fn receive(
    stream: &mut (impl Read + Write),
    roots: &Roots,
    into_folder: bool,
    notify: &dyn Fn(&str),
) -> io::Result<()> {
    let outcome = (|| -> io::Result<(String, PathBuf)> {
        let dir = if into_folder {
            let target = resolve(roots, &get_str(stream)?, "")?;
            if !target.is_dir() {
                return Err(io::Error::other("hedef bir klasör değil"));
            }
            target
        } else {
            fs::create_dir_all(&roots.inbox)?;
            roots.inbox.clone()
        };
        let name = safe_name(&get_str(stream)?);
        let mut size = [0u8; 8];
        stream.read_exact(&mut size)?;
        if !into_folder {
            let mut carry = [0u8; 1];
            stream.read_exact(&mut carry)?;
        }
        let saved = save_stream(stream, &dir, &name, u64::from_le_bytes(size))?;
        Ok((name, saved))
    })();
    match outcome {
        Ok((name, saved)) => {
            stream.write_all(&[1])?;
            notify(&format!("Alındı: {name} ({})", saved.display()));
            Ok(())
        }
        Err(error) => {
            let _ = stream.write_all(&[0]);
            notify(&format!("Dosya alınamadı: {error}"));
            Err(error)
        }
    }
}

fn save_stream(
    stream: &mut impl Read,
    dir: &Path,
    name: &str,
    total: u64,
) -> io::Result<PathBuf> {
    let dest = unique(dir, name);
    let part = dest.with_file_name(format!(
        "{}.part",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut file = File::create(&part)?;
    let mut buffer = vec![0u8; CHUNK];
    let mut got = 0u64;
    while got < total {
        let want = ((total - got) as usize).min(buffer.len());
        let read = match stream.read(&mut buffer[..want]) {
            Ok(0) | Err(_) => {
                let _ = fs::remove_file(&part);
                return Err(io::Error::other("bağlantı koptu"));
            }
            Ok(read) => read,
        };
        file.write_all(&buffer[..read])?;
        got += read as u64;
    }
    file.flush()?;
    drop(file);
    fs::rename(&part, &dest)?;
    Ok(dest)
}

pub fn send<S: Read + Write>(
    stream: &mut S,
    name: &str,
    size: u64,
    source: &mut impl Read,
) -> io::Result<()> {
    stream.write_all(MAGIC)?;
    put_str(stream, name)?;
    stream.write_all(&size.to_le_bytes())?;
    stream.write_all(&[0])?;
    let mut buffer = vec![0u8; CHUNK];
    let mut left = size;
    while left > 0 {
        let want = (left as usize).min(buffer.len());
        let read = source.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(io::Error::other("dosya okunurken kısaldı"));
        }
        stream.write_all(&buffer[..read])?;
        left -= read as u64;
    }
    let mut ack = [0u8; 1];
    stream.read_exact(&mut ack)?;
    if ack[0] == 1 {
        Ok(())
    } else {
        Err(io::Error::other("bilgisayar dosyayı kabul etmedi"))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn start(roots: Roots, allowed: bool) -> (UnixStream, thread::JoinHandle<io::Result<()>>) {
        let (client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || serve_connection(&mut server, &roots, allowed, &|_| {}));
        (client, handle)
    }

    fn layout() -> (tempfile::TempDir, Roots) {
        let dir = tempfile::tempdir().unwrap();
        let share = dir.path().join("depo");
        fs::create_dir_all(share.join("alt")).unwrap();
        fs::write(share.join("not.txt"), b"merhaba").unwrap();
        fs::write(share.join(".gizli"), b"x").unwrap();
        fs::write(dir.path().join("disarida.txt"), b"x").unwrap();
        let inbox = share.join("Download/Mause");
        (dir, Roots { share, inbox })
    }

    fn list(roots: &Roots, base: &str, child: &str) -> (String, Vec<(String, bool, u64)>) {
        let (mut client, handle) = start(
            Roots {
                share: roots.share.clone(),
                inbox: roots.inbox.clone(),
            },
            true,
        );
        client.write_all(LIST).unwrap();
        put_str(&mut client, base).unwrap();
        put_str(&mut client, child).unwrap();
        let mut status = [0u8; 1];
        client.read_exact(&mut status).unwrap();
        assert_eq!(status[0], 1);
        let path = get_str(&mut client).unwrap();
        let mut count = [0u8; 4];
        client.read_exact(&mut count).unwrap();
        let mut found = Vec::new();
        for _ in 0..u32::from_le_bytes(count) {
            let name = get_str(&mut client).unwrap();
            let mut tail = [0u8; 17];
            client.read_exact(&mut tail).unwrap();
            found.push((
                name,
                tail[0] == 1,
                u64::from_le_bytes(tail[1..9].try_into().unwrap()),
            ));
        }
        handle.join().unwrap().unwrap();
        (path, found)
    }

    #[test]
    fn klasor_listesi_gizlileri_atlar_ve_dizinleri_one_alir() {
        let (_dir, roots) = layout();
        let (path, entries) = list(&roots, "", "");
        assert_eq!(
            path,
            fs::canonicalize(&roots.share).unwrap().to_string_lossy()
        );
        assert_eq!(
            entries,
            [("alt".to_owned(), true, 0), ("not.txt".to_owned(), false, 7)]
        );
    }

    #[test]
    fn ust_klasor_depo_disina_cikmaz() {
        let (_dir, roots) = layout();
        let (path, _) = list(&roots, &roots.share.to_string_lossy(), "..");
        assert_eq!(
            path,
            fs::canonicalize(&roots.share).unwrap().to_string_lossy()
        );
        let (name, entries) = list(&roots, DISKS, "");
        assert_eq!(name, DISKS);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn dosya_indirilir_ve_yol_kacisi_reddedilir() {
        let (_dir, roots) = layout();
        let base = roots.share.to_string_lossy().into_owned();
        let (mut client, handle) = start(
            Roots {
                share: roots.share.clone(),
                inbox: roots.inbox.clone(),
            },
            true,
        );
        client.write_all(GET).unwrap();
        put_str(&mut client, &base).unwrap();
        put_str(&mut client, "not.txt").unwrap();
        let mut head = [0u8; 9];
        client.read_exact(&mut head).unwrap();
        assert_eq!(head[0], 1);
        assert_eq!(u64::from_le_bytes(head[1..].try_into().unwrap()), 7);
        let mut body = [0u8; 7];
        client.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"merhaba");
        handle.join().unwrap().unwrap();

        let (mut client, handle) = start(
            Roots {
                share: roots.share.clone(),
                inbox: roots.inbox.clone(),
            },
            true,
        );
        client.write_all(GET).unwrap();
        put_str(&mut client, &base).unwrap();
        put_str(&mut client, "../disarida.txt").unwrap();
        let mut status = [0u8; 1];
        client.read_exact(&mut status).unwrap();
        assert_eq!(status[0], 0);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn dosya_klasore_ve_gelen_kutusuna_yuklenir() {
        let (_dir, roots) = layout();
        let alt = roots.share.join("alt");
        let (mut client, handle) = start(
            Roots {
                share: roots.share.clone(),
                inbox: roots.inbox.clone(),
            },
            true,
        );
        client.write_all(PUT).unwrap();
        put_str(&mut client, &alt.to_string_lossy()).unwrap();
        put_str(&mut client, "../yeni.txt").unwrap();
        client.write_all(&5u64.to_le_bytes()).unwrap();
        client.write_all(b"12345").unwrap();
        let mut ack = [0u8; 1];
        client.read_exact(&mut ack).unwrap();
        assert_eq!(ack[0], 1);
        handle.join().unwrap().unwrap();
        assert_eq!(fs::read(alt.join("yeni.txt")).unwrap(), b"12345");

        let (mut client, handle) = start(
            Roots {
                share: roots.share.clone(),
                inbox: roots.inbox.clone(),
            },
            true,
        );
        let mut source: &[u8] = b"telefondan";
        send(&mut client, "gelen.bin", 10, &mut source).unwrap();
        handle.join().unwrap().unwrap();
        assert_eq!(
            fs::read(roots.inbox.join("gelen.bin")).unwrap(),
            b"telefondan"
        );
    }

    #[test]
    fn izin_kapaliysa_hicbir_sey_acilmaz() {
        let (_dir, roots) = layout();
        let (mut client, handle) = start(roots, false);
        client.write_all(LIST).unwrap();
        put_str(&mut client, "").unwrap();
        put_str(&mut client, "").unwrap();
        let mut status = [0u8; 1];
        client.read_exact(&mut status).unwrap();
        assert_eq!(status[0], 0);
        assert!(get_str(&mut client).unwrap().contains("izin vermiyor"));
        handle.join().unwrap().unwrap();
    }
}
