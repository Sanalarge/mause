use super::*;
use crate::test_support::{TransferPair, wait_transfers};
use std::io::Cursor;

fn tree(parent: &Path) -> PathBuf {
    let root = parent.join("Klasör ğüş");
    fs::create_dir_all(root.join("alt/boş klasör")).unwrap();
    fs::create_dir(root.join(".gizli")).unwrap();
    fs::write(root.join(".gizli/.ayar"), b"hidden").unwrap();
    fs::write(root.join("alt/veri.bin"), vec![42; 600_000]).unwrap();
    fs::write(root.join("sıfır.txt"), []).unwrap();
    root
}

fn check(root: &Path) {
    assert!(root.join("alt/boş klasör").is_dir());
    assert_eq!(fs::read(root.join(".gizli/.ayar")).unwrap(), b"hidden");
    assert_eq!(
        fs::read(root.join("alt/veri.bin")).unwrap(),
        vec![42; 600_000]
    );
    assert_eq!(fs::metadata(root.join("sıfır.txt")).unwrap().len(), 0);
}

#[test]
fn folder_push_preserves_structure_empty_and_hidden_entries_and_collision() {
    let pair = TransferPair::new();
    let temp = tempfile::tempdir().unwrap();
    let source = tree(temp.path());
    send_file(pair.sender.clone(), source.clone(), true);
    assert!(matches!(
        wait_transfers(&pair.sender, 1)[0].status,
        Status::Done(_)
    ));
    let received = wait_transfers(&pair.receiver, 1);
    let Status::Done(path) = &received[0].status else {
        panic!("folder receive failed")
    };
    check(Path::new(path));
    assert_eq!(received[0].done, 600_006);
    assert_eq!(received[0].total, 600_006);
    assert_eq!(
        pair.receiver.last_carry.lock().unwrap().as_slice(),
        &[PathBuf::from(path)]
    );
    fs::write(Path::new(path).join("keep.txt"), b"keep").unwrap();
    send_file(pair.sender.clone(), source, false);
    assert!(matches!(
        wait_transfers(&pair.sender, 2)[1].status,
        Status::Done(_)
    ));
    let second = wait_transfers(&pair.receiver, 2);
    let Status::Done(other) = &second[1].status else {
        panic!("second receive failed")
    };
    assert_ne!(other, path);
    check(Path::new(other));
    assert_eq!(fs::read(Path::new(path).join("keep.txt")).unwrap(), b"keep");
}

#[test]
fn folder_upload_and_download_use_selected_destinations_and_keep_empty_root() {
    let pair = TransferPair::new();
    let temp = tempfile::tempdir().unwrap();
    let source = tree(temp.path());
    let remote = temp.path().join("remote");
    fs::create_dir(&remote).unwrap();
    let local = temp.path().join("local");
    fs::create_dir(&local).unwrap();
    super::upload(
        pair.sender.clone(),
        source,
        false,
        Some(remote.to_string_lossy().into()),
        || {},
    );
    assert!(matches!(
        wait_transfers(&pair.sender, 1)[0].status,
        Status::Done(_)
    ));
    wait_transfers(&pair.receiver, 1);
    check(&remote.join("Klasör ğüş"));
    super::download(
        pair.sender.clone(),
        remote.to_string_lossy().into(),
        "Klasör ğüş".into(),
        local.clone(),
        || {},
    );
    assert!(matches!(
        wait_transfers(&pair.sender, 2)[1].status,
        Status::Done(_)
    ));
    check(&local.join("Klasör ğüş"));
    let empty = temp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    send_file(pair.sender.clone(), empty, false);
    assert!(matches!(
        wait_transfers(&pair.sender, 3)[2].status,
        Status::Done(_)
    ));
    let received = wait_transfers(&pair.receiver, 2);
    let Status::Done(path) = &received[1].status else {
        panic!("empty folder receive failed")
    };
    assert!(Path::new(path).is_dir());
    assert_eq!(fs::read_dir(path).unwrap().count(), 0);
}

#[test]
fn folder_transfers_require_file_permission() {
    let pair = TransferPair::new();
    pair.receiver.perms.lock().unwrap().files_in = false;
    let temp = tempfile::tempdir().unwrap();
    let source = tree(temp.path());
    send_file(pair.sender.clone(), source, false);
    assert!(matches!(
        wait_transfers(&pair.sender, 1)[0].status,
        Status::Failed(_)
    ));
    super::download(
        pair.sender.clone(),
        temp.path().to_string_lossy().into(),
        "Klasör ğüş".into(),
        temp.path().join("download"),
        || {},
    );
    assert!(matches!(
        wait_transfers(&pair.sender, 2)[1].status,
        Status::Failed(_)
    ));
    assert!(!temp.path().join("download").exists());
    assert_eq!(
        fs::read_dir(pair.receiver.test_transfer.inbox.path())
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn malformed_or_interrupted_tree_cannot_escape_and_leaves_no_partial_folder() {
    let sh = Shared::new(false);
    let dest = tempfile::tempdir().unwrap();
    let id = begin(&sh, "folder", 0, true, String::new());
    for bad in [
        "../escape",
        "/absolute",
        "a/../../escape",
        "C:/escape",
        "a\\escape",
        "CON",
        "dir./file",
        "dir//file",
    ] {
        let mut bytes = 0u64.to_le_bytes().to_vec();
        bytes.extend(1u32.to_le_bytes());
        bytes.push(1);
        put_str(&mut bytes, bad).unwrap();
        bytes.extend(0u64.to_le_bytes());
        assert!(
            save_tree(&sh, &mut Cursor::new(bytes), dest.path(), "folder", id).is_err(),
            "{bad}"
        );
        assert_eq!(fs::read_dir(dest.path()).unwrap().count(), 0);
    }
    let mut bytes = 10u64.to_le_bytes().to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.push(1);
    put_str(&mut bytes, "file.bin").unwrap();
    bytes.extend(10u64.to_le_bytes());
    bytes.extend(b"xx");
    assert!(save_tree(&sh, &mut Cursor::new(bytes), dest.path(), "folder", id).is_err());
    assert_eq!(fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_instead_of_following_them() {
    let sh = Shared::new(false);
    let temp = tempfile::tempdir().unwrap();
    let source = tree(temp.path());
    std::os::unix::fs::symlink(temp.path(), source.join("cycle")).unwrap();
    assert!(manifest(&source, &sh).is_err());
}
