use crate::core::Shared;
use crate::net::{self, Secure};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

pub const FT_PORT: u16 = 24804;
const MAGIC: &[u8; 4] = b"MFT2";
const PUT: &[u8; 4] = b"MFP1";
const LIST: &[u8; 4] = b"MFL1";
const GET: &[u8; 4] = b"MFG1";
const MKDIR: &[u8; 4] = b"MFK1";
const DELETE: &[u8; 4] = b"MFX1";
const MAX_ENTRIES: usize = 5000;
const OLD_PEER: &str =
    "Karşı cihazdaki Mause bu işlemi desteklemiyor; karşıda Mause'u güncelleyin.";
const NO_BROWSE: &str =
    "Karşı PC dosya erişimine izin vermiyor (karşıda Paylaşım → Karşıdan bu bilgisayara → Dosya).";

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified: i64,
}

#[derive(Clone)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, PartialEq)]
pub enum Status {
    Active,
    Done(String),
    Failed(String),
}

#[derive(Clone)]
pub struct Transfer {
    pub id: u64,
    pub name: String,
    pub total: u64,
    pub done: u64,
    pub incoming: bool,
    pub dest: String,
    pub status: Status,
}

fn begin(sh: &Shared, name: &str, total: u64, incoming: bool, dest: String) -> u64 {
    let id = sh.next_id.fetch_add(1, Ordering::Relaxed);
    let mut t = sh.transfers.lock().unwrap();
    t.push(Transfer {
        id,
        name: name.into(),
        total,
        done: 0,
        incoming,
        dest,
        status: Status::Active,
    });
    let n = t.len();
    if n > 30 {
        t.drain(..n - 30);
    }
    #[cfg(test)]
    sh.test_transfer.changed.notify_all();
    id
}

fn update(sh: &Shared, id: u64, f: impl FnOnce(&mut Transfer)) {
    if let Some(t) = sh.transfers.lock().unwrap().iter_mut().find(|t| t.id == id) {
        f(t);
        #[cfg(test)]
        sh.test_transfer.changed.notify_all();
    }
}

pub fn inbox() -> PathBuf {
    crate::platform::downloads_dir().join("Mause")
}

fn safe_name(n: &str) -> String {
    let base = n.rsplit(['/', '\\']).next().unwrap_or("dosya");
    let base: String = base.chars().filter(|c| !c.is_control()).collect();
    let base = base.trim().trim_start_matches('.');
    if base.is_empty() {
        "dosya".into()
    } else {
        base.chars().take(200).collect()
    }
}

fn unique(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

pub fn home_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

pub fn resolve(base: &str, child: &str) -> io::Result<PathBuf> {
    let base = if base.is_empty() {
        home_dir()
    } else {
        PathBuf::from(base)
    };
    let p = if child.is_empty() {
        base
    } else {
        base.join(child)
    };
    let p = fs::canonicalize(p)?;
    Ok(match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(s) if !s.starts_with("UNC") => PathBuf::from(s),
        _ => p,
    })
}

pub const DISKS: &str = "Diskler";

pub fn make_dir(dir: &str, name: &str) -> io::Result<PathBuf> {
    let path = resolve(dir, "")?.join(safe_name(name));
    if fs::symlink_metadata(&path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "bu adla bir öğe zaten var",
        ));
    }
    fs::create_dir(&path)?;
    Ok(path)
}

pub fn remove(dir: &str, name: &str) -> io::Result<()> {
    let path = resolve(dir, "")?.join(safe_name(name));
    if fs::symlink_metadata(&path)?.is_dir() {
        fs::remove_dir_all(&path)
    } else {
        fs::remove_file(&path)
    }
}

pub fn listing(base: &str, child: &str) -> io::Result<(String, Vec<Entry>)> {
    if base == DISKS && (child.is_empty() || child == "..") {
        let entries = crate::platform::mounts()
            .into_iter()
            .map(|(_, p)| Entry {
                name: p.to_string_lossy().into_owned(),
                dir: true,
                size: 0,
                modified: 0,
            })
            .collect();
        return Ok((DISKS.to_owned(), entries));
    }
    let p = resolve(base, child)?;
    Ok((p.to_string_lossy().into_owned(), list_dir(&p)?))
}

pub fn list_dir(path: &Path) -> io::Result<Vec<Entry>> {
    let mut v: Vec<Entry> = fs::read_dir(path)?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = fs::metadata(e.path()).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            Some(Entry {
                name,
                dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified,
            })
        })
        .take(MAX_ENTRIES)
        .collect();
    v.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(v)
}

fn put_str(w: &mut impl Write, s: &str) -> io::Result<()> {
    w.write_all(&(s.len() as u16).to_le_bytes())?;
    w.write_all(s.as_bytes())
}

fn get_str(r: &mut impl Read) -> io::Result<String> {
    let mut l = [0u8; 2];
    r.read_exact(&mut l)?;
    let mut b = vec![0u8; u16::from_le_bytes(l) as usize];
    r.read_exact(&mut b)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

fn connect(sh: &Shared) -> io::Result<Secure> {
    let ip = sh
        .peer_ip
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| io::Error::other("karşı PC ile bağlantı yok"))?;
    let secret = sh
        .peer_secrets
        .lock()
        .unwrap()
        .get(&ip)
        .cloned()
        .ok_or_else(|| io::Error::other("oturum anahtarı yok"))?;
    connect_to(sh, &ip, &secret)
}

pub(crate) fn connect_to(sh: &Shared, ip: &str, secret: &str) -> io::Result<Secure> {
    #[cfg(not(test))]
    let _ = sh;
    #[cfg(test)]
    let port = sh.test_transfer.port.load(Ordering::Relaxed);
    #[cfg(not(test))]
    let port = FT_PORT;
    let addr = (ip, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::other("adres"))?;
    let raw = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
    raw.set_nodelay(true).ok();
    let s = net::handshake(raw, true, secret)?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    s.set_write_timeout(Some(Duration::from_secs(60)))?;
    Ok(s)
}

fn read_status(s: &mut impl Read) -> io::Result<()> {
    let mut b = [0u8; 1];
    s.read_exact(&mut b)?;
    if b[0] == 1 {
        Ok(())
    } else {
        Err(io::Error::other(get_str(s)?))
    }
}

fn write_error(s: &mut impl Write, msg: &str) -> io::Result<()> {
    s.write_all(&[0])?;
    put_str(s, msg)
}

pub fn send_file(sh: Arc<Shared>, path: PathBuf, carry: bool) {
    push(sh, path, carry, None, || {});
}

pub fn upload(
    sh: Arc<Shared>,
    path: PathBuf,
    remote_dir: String,
    done: impl FnOnce() + Send + 'static,
) {
    push(sh, path, false, Some(remote_dir), done);
}

fn push(
    sh: Arc<Shared>,
    path: PathBuf,
    carry: bool,
    dest: Option<String>,
    done: impl FnOnce() + Send + 'static,
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dosya".into());
    if sh.peer_ip.lock().unwrap().is_none() {
        sh.log(format!(
            "'{name}' gönderilemedi: karşı PC ile bağlantı yok."
        ));
        return;
    }
    let meta = match fs::metadata(&path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => {
            sh.log(format!(
                "'{name}' bir klasör; klasörler desteklenmiyor, sıkıştırıp gönderin."
            ));
            return;
        }
        Err(e) => {
            sh.log(format!("'{name}' okunamadı: {e}"));
            return;
        }
    };
    let target = match &dest {
        Some(dir) => format!("Karşı · {dir}"),
        None => "Karşı · İndirilenler/Mause".to_owned(),
    };
    let id = begin(&sh, &name, meta.len(), false, target);
    thread::spawn(move || {
        let r = (|| -> io::Result<()> {
            let mut s = connect(&sh)?;
            match &dest {
                Some(dir) => {
                    s.write_all(PUT)?;
                    put_str(&mut s, dir)?;
                    put_str(&mut s, &name)?;
                    s.write_all(&meta.len().to_le_bytes())?;
                }
                None => {
                    s.write_all(MAGIC)?;
                    put_str(&mut s, &name)?;
                    s.write_all(&meta.len().to_le_bytes())?;
                    s.write_all(&[carry as u8])?;
                }
            }
            let mut f = File::open(&path)?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut sent = 0u64;
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                s.write_all(&buf[..n])?;
                sent += n as u64;
                update(&sh, id, |t| t.done = sent);
                if sh.stopping() {
                    return Err(io::Error::other("durduruldu"));
                }
            }
            let mut ack = [0u8; 1];
            s.read_exact(&mut ack)?;
            if ack[0] != 1 {
                return Err(io::Error::other("karşı taraf reddetti"));
            }
            Ok(())
        })();
        match r {
            Ok(()) => {
                update(&sh, id, |t| {
                    t.status = Status::Done("karşı PC'ye gönderildi".into())
                });
                sh.log(format!("Gönderildi: {name}"));
            }
            Err(e) => {
                update(&sh, id, |t| t.status = Status::Failed(e.to_string()));
                sh.log(format!("'{name}' gönderilemedi: {e}"));
            }
        }
        done();
    });
}

fn remote_command(sh: &Shared, magic: &[u8; 4], dir: &str, name: &str) -> io::Result<()> {
    let mut s = connect(sh)?;
    s.write_all(magic)?;
    put_str(&mut s, dir)?;
    put_str(&mut s, name)?;
    read_status(&mut s).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            io::Error::other(OLD_PEER)
        } else {
            e
        }
    })
}

pub fn remote_mkdir(sh: &Shared, dir: &str, name: &str) -> io::Result<()> {
    remote_command(sh, MKDIR, dir, name)
}

pub fn remote_delete(sh: &Shared, dir: &str, name: &str) -> io::Result<()> {
    remote_command(sh, DELETE, dir, name)
}

pub fn list_remote(sh: &Shared, base: &str, child: &str) -> io::Result<Listing> {
    let mut s = connect(sh)?;
    s.write_all(LIST)?;
    put_str(&mut s, base)?;
    put_str(&mut s, child)?;
    read_status(&mut s)?;
    let path = get_str(&mut s)?;
    let mut n = [0u8; 4];
    s.read_exact(&mut n)?;
    let n = (u32::from_le_bytes(n) as usize).min(MAX_ENTRIES);
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let name = get_str(&mut s)?;
        let mut b = [0u8; 17];
        s.read_exact(&mut b)?;
        entries.push(Entry {
            name,
            dir: b[0] == 1,
            size: u64::from_le_bytes(b[1..9].try_into().unwrap()),
            modified: i64::from_le_bytes(b[9..17].try_into().unwrap()),
        });
    }
    Ok(Listing { path, entries })
}

pub fn download(
    sh: Arc<Shared>,
    remote_dir: String,
    name: String,
    dest_dir: PathBuf,
    done: impl FnOnce() + Send + 'static,
) {
    let id = begin(
        &sh,
        &name,
        0,
        true,
        format!("Bu PC · {}", dest_dir.display()),
    );
    thread::spawn(move || {
        let r = (|| -> io::Result<PathBuf> {
            let mut s = connect(&sh)?;
            s.write_all(GET)?;
            put_str(&mut s, &remote_dir)?;
            put_str(&mut s, &name)?;
            read_status(&mut s)?;
            let mut sz = [0u8; 8];
            s.read_exact(&mut sz)?;
            let total = u64::from_le_bytes(sz);
            update(&sh, id, |t| t.total = total);
            save_stream(&sh, &mut s, &dest_dir, &safe_name(&name), total, id)
        })();
        match r {
            Ok(dest) => {
                update(&sh, id, |t| {
                    t.status = Status::Done(dest.to_string_lossy().into_owned())
                });
                sh.log(format!("İndirildi: {}", dest.display()));
            }
            Err(e) => {
                update(&sh, id, |t| t.status = Status::Failed(e.to_string()));
                sh.log(format!("'{name}' indirilemedi: {e}"));
            }
        }
        done();
    });
}

fn save_stream(
    sh: &Shared,
    s: &mut impl Read,
    dir: &Path,
    name: &str,
    total: u64,
    id: u64,
) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let dest = unique(dir, name);
    let part = dest.with_extension(format!(
        "{}.part",
        dest.extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let mut f = File::create(&part)?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut got = 0u64;
    while got < total {
        let want = ((total - got) as usize).min(buf.len());
        let n = match s.read(&mut buf[..want]) {
            Ok(0) | Err(_) => {
                let _ = fs::remove_file(&part);
                return Err(io::Error::other("bağlantı koptu"));
            }
            Ok(n) => n,
        };
        f.write_all(&buf[..n])?;
        got += n as u64;
        update(sh, id, |t| t.done = got);
    }
    f.flush()?;
    drop(f);
    fs::rename(&part, &dest)?;
    Ok(dest)
}

pub fn serve(sh: Arc<Shared>) {
    let l = match TcpListener::bind(("0.0.0.0", FT_PORT)) {
        Ok(l) => l,
        Err(e) => {
            sh.log(format!("Dosya alıcısı başlatılamadı (port {FT_PORT}): {e}"));
            return;
        }
    };
    let _ = l.set_nonblocking(true);
    let _ = serve_listener(sh, l);
}

pub(crate) fn serve_listener(sh: Arc<Shared>, l: TcpListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !sh.stopping() {
            match l.accept() {
                Ok((s, from)) => {
                    let Some(secret2) = sh
                        .peer_secrets
                        .lock()
                        .unwrap()
                        .get(&from.ip().to_string())
                        .cloned()
                    else {
                        continue;
                    };
                    let sh2 = sh.clone();
                    thread::spawn(move || receive(sh2, s, secret2));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(150))
                }
                Err(_) => thread::sleep(Duration::from_millis(500)),
            }
        }
    })
}

fn receive(sh: Arc<Shared>, raw: TcpStream, secret: String) {
    let _ = raw.set_nonblocking(false);
    let mut s: Secure = match net::handshake(raw, false, &secret) {
        Ok(s) => s,
        Err(e) => {
            sh.log(format!("Dosya bağlantısı reddedildi: {e}"));
            return;
        }
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(60)));
    let mut m = [0u8; 4];
    if s.read_exact(&mut m).is_err() {
        return;
    }
    let ip = s
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    let perms = sh.perms_for_ip(&ip);
    match &m {
        crate::clipboard_sync::MAGIC => crate::clipboard_sync::receive(&sh, &ip, &mut s),
        MAGIC if perms.files_in => receive_file(sh, s, None),
        PUT if perms.files_in => receive_file(sh, s, Some(())),
        LIST if perms.files_in => {
            let _ = serve_list(&mut s);
        }
        GET if perms.files_in => serve_get(&sh, &mut s),
        MKDIR if perms.files_in => serve_change(&sh, &mut s, true),
        DELETE if perms.files_in => serve_change(&sh, &mut s, false),
        LIST | GET | PUT | MKDIR | DELETE => {
            let _ = get_str(&mut s)
                .and_then(|_| get_str(&mut s))
                .and_then(|_| write_error(&mut s, NO_BROWSE));
        }
        _ => {}
    }
}

fn serve_change(sh: &Shared, s: &mut Secure, create: bool) {
    let r = (|| -> io::Result<()> {
        let (dir, name) = (get_str(s)?, get_str(s)?);
        let done = if create {
            make_dir(&dir, &name).map(|p| format!("Karşı PC klasör oluşturdu: {}", p.display()))
        } else {
            remove(&dir, &name).map(|()| format!("Karşı PC sildi: {dir}/{name}"))
        };
        match done {
            Ok(message) => {
                sh.log(message);
                s.write_all(&[1])
            }
            Err(e) => write_error(s, &e.to_string()),
        }
    })();
    if let Err(e) = r {
        sh.log(format!("Dosya işlemi yanıtlanamadı: {e}"));
    }
}

fn serve_list(s: &mut Secure) -> io::Result<()> {
    let (base, child) = (get_str(s)?, get_str(s)?);
    let (path, entries) = match listing(&base, &child) {
        Ok(v) => v,
        Err(e) => return write_error(s, &format!("klasör açılamadı: {e}")),
    };
    let mut out = vec![1u8];
    put_str(&mut out, &path)?;
    out.extend((entries.len() as u32).to_le_bytes());
    for e in &entries {
        put_str(&mut out, &e.name)?;
        out.push(e.dir as u8);
        out.extend(e.size.to_le_bytes());
        out.extend(e.modified.to_le_bytes());
    }
    s.write_all(&out)
}

fn serve_get(sh: &Shared, s: &mut Secure) {
    let r = (|| -> io::Result<()> {
        let (dir, name) = (get_str(s)?, get_str(s)?);
        let path = match resolve(&dir, &safe_name(&name)) {
            Ok(p) if p.is_file() => p,
            Ok(_) => return write_error(s, "bu bir dosya değil"),
            Err(e) => return write_error(s, &format!("dosya açılamadı: {e}")),
        };
        let mut f = match File::open(&path) {
            Ok(f) => f,
            Err(e) => return write_error(s, &format!("dosya açılamadı: {e}")),
        };
        let total = f.metadata()?.len();
        s.write_all(&[1])?;
        s.write_all(&total.to_le_bytes())?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut left = total;
        while left > 0 {
            let want = (left as usize).min(buf.len());
            let n = f.read(&mut buf[..want])?;
            if n == 0 {
                return Err(io::Error::other("dosya okunurken kısaldı"));
            }
            s.write_all(&buf[..n])?;
            left -= n as u64;
        }
        sh.log(format!("Karşı PC indirdi: {}", path.display()));
        Ok(())
    })();
    if let Err(e) = r {
        sh.log(format!("Karşı PC'ye dosya gönderilemedi: {e}"));
    }
}

fn receive_file(sh: Arc<Shared>, mut s: Secure, put: Option<()>) {
    let mut name = String::from("dosya");
    let mut id = None;
    let mut carry = false;
    let r = (|| -> io::Result<PathBuf> {
        let dir = match put {
            Some(()) => resolve(&get_str(&mut s)?, "")?,
            None => {
                #[cfg(test)]
                {
                    sh.test_transfer.inbox.path().to_path_buf()
                }
                #[cfg(not(test))]
                {
                    inbox()
                }
            }
        };
        name = safe_name(&get_str(&mut s)?);
        let mut sz = [0u8; 8];
        s.read_exact(&mut sz)?;
        let total = u64::from_le_bytes(sz);
        if put.is_none() {
            let mut fl = [0u8; 1];
            s.read_exact(&mut fl)?;
            carry = fl[0] == 1;
        }
        let i = begin(
            &sh,
            &name,
            total,
            true,
            format!("Bu PC · {}", dir.display()),
        );
        id = Some(i);
        let dest = save_stream(&sh, &mut s, &dir, &name, total, i)?;
        s.write_all(&[1])?;
        Ok(dest)
    })();
    match r {
        Ok(dest) => {
            if let Some(i) = id {
                update(&sh, i, |t| {
                    t.status = Status::Done(dest.to_string_lossy().into_owned())
                });
            }
            sh.log(format!("Alındı: {}", dest.display()));
            if carry {
                let list = {
                    let mut r = sh.recent.lock().unwrap();
                    if r.0.elapsed() > Duration::from_secs(5) {
                        r.1.clear();
                    }
                    r.0 = std::time::Instant::now();
                    r.1.push(dest.clone());
                    r.1.clone()
                };
                *sh.last_carry.lock().unwrap() = list.clone();
                thread::spawn(move || crate::clip::set_files(&list));
            }
            #[cfg(not(test))]
            crate::platform::notify(
                if carry {
                    "Dosya geldi: Ctrl+V ile yapıştırın"
                } else {
                    "Dosya alındı"
                },
                &format!(
                    "{}\n{}",
                    name,
                    dest.parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                ),
            );
        }
        Err(e) => {
            let _ = s.write_all(&[0]);
            if let Some(i) = id {
                update(&sh, i, |t| t.status = Status::Failed(e.to_string()));
            }
            sh.log(format!("Dosya alınamadı: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TransferPair, wait_transfers};

    #[test]
    fn karsida_klasor_olustur_ve_sil() {
        let pair = TransferPair::new();
        let (a, b) = (&pair.sender, &pair.receiver);
        b.perms.lock().unwrap().files_in = true;
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_string_lossy().into_owned();
        fs::write(root.path().join("eski.txt"), b"x").unwrap();

        remote_mkdir(a, &dir, "Yeni klasör").unwrap();
        assert!(root.path().join("Yeni klasör").is_dir());
        assert!(remote_mkdir(a, &dir, "Yeni klasör").is_err());
        fs::write(root.path().join("Yeni klasör").join("ic.txt"), b"y").unwrap();

        remote_delete(a, &dir, "Yeni klasör").unwrap();
        remote_delete(a, &dir, "eski.txt").unwrap();
        assert!(!root.path().join("Yeni klasör").exists());
        assert!(!root.path().join("eski.txt").exists());
        assert!(remote_delete(a, &dir, "yok.txt").is_err());
        remote_mkdir(a, &dir, "../kacis").unwrap();
        assert!(root.path().join("kacis").is_dir());
        assert!(!root.path().parent().unwrap().join("kacis").exists());

        b.perms.lock().unwrap().files_in = false;
        assert!(remote_mkdir(a, &dir, "izinsiz").is_err());
        assert!(!root.path().join("izinsiz").exists());
    }

    #[test]
    fn klasor_listele_indir_yukle() {
        let pair = TransferPair::new();
        let (a, b) = (&pair.sender, &pair.receiver);
        b.perms.lock().unwrap().files_in = true;
        let root = tempfile::tempdir().unwrap();
        let (far, near) = (root.path().join("karsi"), root.path().join("bu"));
        fs::create_dir_all(far.join("alt")).unwrap();
        fs::create_dir_all(&near).unwrap();
        fs::write(far.join("not.txt"), b"merhaba").unwrap();
        fs::write(far.join(".gizli"), b"x").unwrap();

        let l = list_remote(a, &far.to_string_lossy(), "").unwrap();
        let names: Vec<(&str, bool)> = l.entries.iter().map(|e| (e.name.as_str(), e.dir)).collect();
        assert_eq!(names, [("alt", true), ("not.txt", false)]);
        assert_eq!(l.entries[1].size, 7);
        let up = list_remote(a, &l.path, "..").unwrap();
        assert!(up.entries.iter().any(|e| e.name == "karsi" && e.dir));

        download(
            a.clone(),
            l.path.clone(),
            "not.txt".into(),
            near.clone(),
            || {},
        );
        assert!(matches!(wait_transfers(a, 1)[0].status, Status::Done(_)));
        assert_eq!(fs::read(near.join("not.txt")).unwrap(), b"merhaba");

        let alt = Path::new(&l.path)
            .join("alt")
            .to_string_lossy()
            .into_owned();
        upload(a.clone(), near.join("not.txt"), alt, || {});
        assert!(
            wait_transfers(a, 2)
                .iter()
                .all(|t| matches!(t.status, Status::Done(_)))
        );
        assert_eq!(fs::read(far.join("alt/not.txt")).unwrap(), b"merhaba");

        fs::write(near.join("disarida.txt"), b"x").unwrap();
        download(
            a.clone(),
            l.path.clone(),
            "../bu/disarida.txt".into(),
            root.path().to_path_buf(),
            || {},
        );
        assert!(matches!(
            wait_transfers(a, 3).last().unwrap().status,
            Status::Failed(_)
        ));

        b.perms.lock().unwrap().files_in = false;
        let e = list_remote(a, &l.path, "")
            .err()
            .expect("izinsiz listelendi");
        assert!(e.to_string().contains("izin vermiyor"), "{e}");
    }

    #[test]
    fn dosya_gonder_al() {
        let pair = TransferPair::new();
        let (a, b) = (&pair.sender, &pair.receiver);
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("dosya-test.bin");
        let data: Vec<u8> = (0..20_000_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &data).unwrap();
        send_file(a.clone(), src.clone(), false);
        assert!(matches!(wait_transfers(a, 1)[0].status, Status::Done(_)));
        let received = wait_transfers(b, 1);
        let Status::Done(path) = &received[0].status else {
            panic!("alım başarısız")
        };
        assert!(Path::new(path).starts_with(b.test_transfer.inbox.path()));
        assert_eq!(fs::read(path).unwrap(), data, "içerik farklı");

        a.peer_secrets
            .lock()
            .unwrap()
            .insert("127.0.0.1".into(), "yanlis".into());
        send_file(a.clone(), src, false);
        assert!(
            matches!(wait_transfers(a, 2)[1].status, Status::Failed(_)),
            "yanlış parola kabul edildi"
        );
    }
}
