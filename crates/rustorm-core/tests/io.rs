//! File creation modes, backups and atomic writes (step 3 of plan basilisk).
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use rustorm_core::{backup_path, write_text, ConfigFile, WriteOptions};

fn mode(p: &std::path::Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn missing_file_is_created_0600_in_a_0700_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("dotssh").join("config");
    let mut f = ConfigFile::load(&path).unwrap();
    assert!(!f.existed);
    f.config
        .insert_host(None, rustorm_core::HostBlock::new(&["a".into()]));
    f.save(WriteOptions::default()).unwrap();
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().unwrap()), 0o700);
    assert_eq!(fs::read_to_string(&path).unwrap(), "Host a\n");
    assert!(!backup_path(&path).exists(), "nothing to back up on create");
}

#[test]
fn every_write_backs_up_the_pre_write_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config");
    fs::write(&path, "Host a\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    write_text(&path, "Host b\n", WriteOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), "Host a\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "Host b\n");
    assert_eq!(mode(&path), 0o640, "existing mode is preserved");
    write_text(&path, "Host c\n", WriteOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), "Host b\n");
    write_text(&path, "Host d\n", WriteOptions { no_backup: true }).unwrap();
    assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), "Host b\n");
}

#[test]
fn failed_write_in_read_only_directory_leaves_original_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("ro");
    fs::create_dir(&dir).unwrap();
    let path = dir.join("config");
    fs::write(&path, "Host keep\n").unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    let with_backup = write_text(&path, "Host new\n", WriteOptions::default());
    let without_backup = write_text(&path, "Host new\n", WriteOptions { no_backup: true });
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let err = without_backup.unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(with_backup.is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "Host keep\n");
    let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "no temp file left behind");
}

#[test]
fn backup_command_copies_to_named_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config");
    fs::write(&path, "Host a\n").unwrap();
    let f = ConfigFile::load(&path).unwrap();
    let dest = tmp.path().join("copy");
    assert_eq!(f.backup(Some(&dest)).unwrap(), dest);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "Host a\n");
    assert_eq!(f.backup(None).unwrap(), backup_path(&path));
    let missing = ConfigFile::load(tmp.path().join("nope")).unwrap();
    assert_eq!(missing.backup(None).unwrap_err().exit_code(), 3);
}

#[test]
fn symlinked_config_writes_through_to_target() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    fs::write(&real, "Host a\n").unwrap();
    let link = tmp.path().join("config");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    write_text(&link, "Host b\n", WriteOptions::default()).unwrap();
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(&real).unwrap(), "Host b\n");
}
