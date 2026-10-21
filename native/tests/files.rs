use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use filmlab::files::{is_temporary, write_atomic};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("filmlab-files-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn leftovers(dir: &PathBuf) -> Vec<String> {
    fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| is_temporary(n)).collect()
}

#[test]
fn replaces_the_file_whole_and_keeps_its_permissions() {
    let dir = folder("replace");
    let path = dir.join("photo.xmp");
    write_atomic(&path, b"first").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    write_atomic(&path, b"second, longer").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second, longer");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(leftovers(&dir).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn writes_through_a_link_to_the_file_it_names() {
    let dir = folder("link");
    let (real, link) = (dir.join("real.xmp"), dir.join("link.xmp"));
    fs::write(&real, b"old").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    write_atomic(&link, b"new").unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&real).unwrap(), b"new");
    assert!(leftovers(&dir).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_write_leaves_the_old_file_and_no_temporary() {
    let dir = folder("fail");
    let path = dir.join("photo.xmp");
    write_atomic(&path, b"kept").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = write_atomic(&path, b"lost");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"kept");
    assert!(leftovers(&dir).is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn of_quick_writes_in_the_background_the_newest_lands() {
    let dir = folder("soon");
    let path = dir.join("state.json");
    for i in 0..50 {
        filmlab::files::write_atomic_soon(path.clone(), format!("{i}").into_bytes());
    }
    let started = std::time::Instant::now();
    while fs::read_to_string(&path).ok().as_deref() != Some("49") && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(fs::read_to_string(&path).unwrap(), "49");
    assert!(leftovers(&dir).is_empty());
    fs::remove_dir_all(dir).unwrap();
}
