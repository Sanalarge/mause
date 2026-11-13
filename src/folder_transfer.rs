use super::*;
use std::collections::HashSet;
use std::fs::OpenOptions;
#[cfg(test)]
#[path = "folder_transfer_tests.rs"]
mod tests;

const LIMIT: usize = 100_000;
const DEPTH: usize = 128;

struct Item {
    relative: String,
    path: PathBuf,
    size: Option<u64>,
}

fn component(name: &str) -> io::Result<()> {
    let reserved = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
        || matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (reserved.len() == 4
            && (reserved.starts_with("COM") || reserved.starts_with("LPT"))
            && matches!(reserved.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(io::Error::other(
            "Geçersiz veya hedef sistemle uyumsuz klasör/dosya adı.",
        ));
    }
    Ok(())
}

fn relative_path(name: &str) -> io::Result<PathBuf> {
    if name.len() > 30_000 || name.split('/').count() > DEPTH {
        return Err(io::Error::other("Klasör yolu çok uzun."));
    }
    let mut path = PathBuf::new();
    for part in name.split('/') {
        component(part)?;
        path.push(part);
    }
    Ok(path)
}

fn ordinary(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    meta.is_file() || meta.is_dir()
}

fn manifest(root: &Path, sh: &Shared) -> io::Result<(Vec<Item>, u64)> {
    let meta = fs::symlink_metadata(root)?;
    if !meta.is_dir() || !ordinary(&meta) {
        return Err(io::Error::other("Bağlantı veya özel klasör aktarılamaz."));
    }
    let mut items = Vec::new();
    let mut total = 0u64;
    fn walk(
        root: &Path,
        dir: &Path,
        items: &mut Vec<Item>,
        total: &mut u64,
        sh: &Shared,
        depth: usize,
    ) -> io::Result<()> {
        if depth > DEPTH {
            return Err(io::Error::other("Klasörler çok iç içe."));
        }
        for entry in fs::read_dir(dir)? {
            if sh.stopping() {
                return Err(io::Error::other("durduruldu"));
            }
            let path = entry?.path();
            let rel = path.strip_prefix(root).map_err(io::Error::other)?;
            let names = rel
                .iter()
                .map(|s| {
                    s.to_str()
                        .ok_or_else(|| io::Error::other("Dosya adı UTF-8 olarak okunamadı."))
                })
                .collect::<io::Result<Vec<_>>>()?;
            let relative = names.join("/");
            relative_path(&relative)?;
            let meta = fs::symlink_metadata(&path)?;
            if !ordinary(&meta) {
                return Err(io::Error::other(format!(
                    "Bağlantı veya özel dosya aktarılamaz: {relative}"
                )));
            }
            if items.len() >= LIMIT {
                return Err(io::Error::other("Klasörde çok fazla öğe var."));
            }
            let size = meta.is_file().then_some(meta.len());
            *total = total
                .checked_add(size.unwrap_or(0))
                .ok_or_else(|| io::Error::other("Klasör boyutu çok büyük."))?;
            items.push(Item {
                relative,
                path: path.clone(),
                size,
            });
            if meta.is_dir() {
                walk(root, &path, items, total, sh, depth + 1)?;
            }
        }
        Ok(())
    }
    walk(root, root, &mut items, &mut total, sh, 0)?;
    Ok((items, total))
}

fn send_tree(
    sh: &Shared,
    stream: &mut impl Write,
    items: &[Item],
    total: u64,
    id: Option<u64>,
) -> io::Result<()> {
    stream.write_all(&total.to_le_bytes())?;
    stream.write_all(&(items.len() as u32).to_le_bytes())?;
    let mut sent = 0;
    let mut buf = vec![0; 256 * 1024];
    for item in items {
        stream.write_all(&[item.size.is_some() as u8])?;
        put_str(stream, &item.relative)?;
        if let Some(size) = item.size {
            if !ordinary(&fs::symlink_metadata(&item.path)?) {
                return Err(io::Error::other("Kaynak dosya değişti."));
            }
            let mut file = File::open(&item.path)?;
            stream.write_all(&size.to_le_bytes())?;
            let mut left = size;
            while left > 0 {
                if sh.stopping() {
                    return Err(io::Error::other("durduruldu"));
                }
                let want = left.min(buf.len() as u64) as usize;
                let n = file.read(&mut buf[..want])?;
                if n == 0 {
                    return Err(io::Error::other("Kaynak dosya okunurken kısaldı."));
                }
                stream.write_all(&buf[..n])?;
                left -= n as u64;
                sent += n as u64;
                if let Some(id) = id {
                    update(sh, id, |t| t.done = sent);
                }
            }
        }
    }
    Ok(())
}

struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn save_tree(
    sh: &Shared,
    stream: &mut impl Read,
    dir: &Path,
    name: &str,
    id: u64,
) -> io::Result<PathBuf> {
    component(name)?;
    let mut header = [0; 12];
    stream.read_exact(&mut header)?;
    let total = u64::from_le_bytes(header[..8].try_into().unwrap());
    let count = u32::from_le_bytes(header[8..].try_into().unwrap()) as usize;
    if count > LIMIT {
        return Err(io::Error::other("Klasörde çok fazla öğe var."));
    }
    update(sh, id, |t| t.total = total);
    fs::create_dir_all(dir)?;
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let temp = dir.join(format!(
        ".mause-folder-{:032x}.part",
        u128::from_le_bytes(random)
    ));
    fs::create_dir(&temp)?;
    let stage = Stage(temp);
    let mut seen = HashSet::new();
    let mut got = 0u64;
    let mut buf = vec![0; 256 * 1024];
    for _ in 0..count {
        if sh.stopping() {
            return Err(io::Error::other("durduruldu"));
        }
        let mut kind = [0];
        stream.read_exact(&mut kind)?;
        let relative = relative_path(&get_str(stream)?)?;
        if !seen.insert(relative.clone()) {
            return Err(io::Error::other("Klasörde tekrarlanan yol var."));
        }
        let path = stage.0.join(relative);
        match kind[0] {
            0 => fs::create_dir(&path)?,
            1 => {
                let mut size = [0; 8];
                stream.read_exact(&mut size)?;
                let size = u64::from_le_bytes(size);
                if got.checked_add(size).is_none_or(|n| n > total) {
                    return Err(io::Error::other("Klasör boyutu tutmuyor."));
                }
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)?;
                let mut left = size;
                while left > 0 {
                    if sh.stopping() {
                        return Err(io::Error::other("durduruldu"));
                    }
                    let want = left.min(buf.len() as u64) as usize;
                    stream.read_exact(&mut buf[..want])?;
                    file.write_all(&buf[..want])?;
                    left -= want as u64;
                    got += want as u64;
                    update(sh, id, |t| t.done = got);
                }
                file.flush()?;
            }
            _ => return Err(io::Error::other("Geçersiz klasör öğesi.")),
        }
    }
    if got != total {
        return Err(io::Error::other("Klasör eksik alındı."));
    }
    let dest = unique(dir, name);
    fs::rename(&stage.0, &dest)?;
    Ok(dest)
}

pub(super) fn upload(
    sh: Arc<Shared>,
    path: PathBuf,
    carry: bool,
    dir: Option<String>,
    done: impl FnOnce() + Send + 'static,
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "klasör".into());
    let id = begin(
        &sh,
        &name,
        0,
        false,
        format!("Karşı · {}", dir.as_deref().unwrap_or("İndirilenler/Mause")),
    );
    thread::spawn(move || {
        let result = (|| -> io::Result<()> {
            component(&name)?;
            let (items, total) = manifest(&path, &sh)?;
            update(&sh, id, |t| t.total = total);
            let mut s = connect(&sh)?;
            s.write_all(TREE_PUT)?;
            put_str(&mut s, dir.as_deref().unwrap_or(""))?;
            put_str(&mut s, &name)?;
            s.write_all(&[carry as u8])?;
            read_status(&mut s).map_err(|e| {
                io::Error::other(format!(
                    "Klasör aktarımı başlatılamadı: {e}. İki PC'de de güncel Mause kullanın."
                ))
            })?;
            send_tree(&sh, &mut s, &items, total, Some(id))?;
            read_status(&mut s)
        })();
        finish(
            &sh,
            id,
            &name,
            result.map(|()| "karşı PC'ye gönderildi".into()),
        );
        done();
    });
}

fn finish(sh: &Shared, id: u64, name: &str, result: io::Result<String>) {
    match result {
        Ok(dest) => {
            update(sh, id, |t| t.status = Status::Done(dest));
            sh.log(format!("Klasör aktarıldı: {name}"));
        }
        Err(e) => {
            update(sh, id, |t| t.status = Status::Failed(e.to_string()));
            sh.log(format!("'{name}' klasörü aktarılamadı: {e}"));
        }
    }
}

pub(super) fn receive(sh: Arc<Shared>, mut stream: Secure) {
    let mut name = "klasör".to_owned();
    let mut id = None;
    let result = (|| -> io::Result<String> {
        let dir = get_str(&mut stream)?;
        name = get_str(&mut stream)?;
        component(&name)?;
        let mut carry = [0];
        stream.read_exact(&mut carry)?;
        let dir = if dir.is_empty() {
            #[cfg(test)]
            {
                sh.test_transfer.inbox.path().to_path_buf()
            }
            #[cfg(not(test))]
            {
                inbox()
            }
        } else {
            resolve(&dir, "")?
        };
        let transfer = begin(&sh, &name, 0, true, format!("Bu PC · {}", dir.display()));
        id = Some(transfer);
        stream.write_all(&[1])?;
        let dest = save_tree(&sh, &mut stream, &dir, &name, transfer)?;
        stream.write_all(&[1])?;
        if carry[0] == 1 {
            let list = {
                let mut recent = sh.recent.lock().unwrap();
                if recent.0.elapsed() > Duration::from_secs(5) {
                    recent.1.clear();
                }
                recent.0 = std::time::Instant::now();
                recent.1.push(dest.clone());
                recent.1.clone()
            };
            *sh.last_carry.lock().unwrap() = list.clone();
            #[cfg(not(test))]
            thread::spawn(move || crate::clip::set_files(&list));
        }
        Ok(dest.to_string_lossy().into_owned())
    })();
    if let Err(e) = &result {
        let _ = write_error(&mut stream, &e.to_string());
    }
    if let Some(id) = id {
        finish(&sh, id, &name, result);
    }
}

pub(super) fn serve_get(sh: &Shared, stream: &mut Secure) {
    let prepared = (|| -> io::Result<_> {
        let dir = get_str(stream)?;
        let name = get_str(stream)?;
        component(&name)?;
        let path = resolve(&dir, "")?.join(&name);
        manifest(&path, sh)
    })();
    match prepared {
        Ok((items, total)) => {
            let _ = stream
                .write_all(&[1])
                .and_then(|()| send_tree(sh, stream, &items, total, None));
        }
        Err(e) => {
            let _ = write_error(stream, &e.to_string());
        }
    }
}

pub(super) fn download(
    sh: Arc<Shared>,
    dir: String,
    name: String,
    dest: PathBuf,
    done: impl FnOnce() + Send + 'static,
) {
    let id = begin(&sh, &name, 0, true, format!("Bu PC · {}", dest.display()));
    thread::spawn(move || {
        let result = (|| -> io::Result<String> {
            component(&name)?;
            let mut stream = connect(&sh)?;
            stream.write_all(TREE_GET)?;
            put_str(&mut stream, &dir)?;
            put_str(&mut stream, &name)?;
            read_status(&mut stream).map_err(|e| {
                io::Error::other(format!(
                    "Klasör alınamadı: {e}. İki PC'de de güncel Mause kullanın."
                ))
            })?;
            Ok(save_tree(&sh, &mut stream, &dest, &name, id)?
                .to_string_lossy()
                .into_owned())
        })();
        finish(&sh, id, &name, result);
        done();
    });
}
