// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Full-file replacement extracted from m2slice::Replica::persist. Callers
//! serialize writers and keep the destination directory in place. Syncing the
//! directory completes durability of the rename on the supported local FS.
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

pub(crate) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = File::open(path.parent().ok_or_else(|| io::Error::other("no parent"))?)?;
    checkpoint(1)?;
    // Leave fixed .tmp crash remnants alone. RandomState supplies a fresh
    // randomly seeded hasher; exclusive creation is the safety boundary even
    // if a name collides or an entry is planted before open.
    use std::hash::{BuildHasher, Hasher};
    let (temporary, mut file) = loop {
        let mut name = std::collections::hash_map::RandomState::new().build_hasher();
        name.write(path.as_os_str().as_encoded_bytes());
        let temporary = path.with_extension(std::format!("tmp-{:016x}", name.finish()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        // Exercise a short write followed by an I/O error, not only a failure
        // before any bytes reached the temporary file.
        #[cfg(test)]
        if FAULT.with(|fault| fault.get() == (2, false)) {
            file.write_all(&bytes[..bytes.len() / 2])?;
        }
        checkpoint(2)?;
        file.write_all(bytes)?;
        checkpoint(3)?;
        file.sync_all()?;
        checkpoint(4)?;
        fs::rename(&temporary, path)
    })();
    // Close before best-effort removal, preserving the original write/sync/rename error.
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    checkpoint(5)?;
    parent.sync_all()?;
    checkpoint(6)?;
    Ok(())
}

pub(crate) fn checkpoint(_boundary: u8) -> io::Result<()> {
    #[cfg(test)]
    FAULT.with(|fault| {
        let (boundary, crash) = fault.get();
        if boundary == _boundary {
            if crash {
                std::process::exit(77);
            }
            return Err(io::Error::other("injected uncertain I/O"));
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    pub(crate) static FAULT: std::cell::Cell<(u8, bool)> = const { std::cell::Cell::new((0, false)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn directory() -> std::path::PathBuf {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(std::format!(
            "safemesh-replace-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        directory
    }

    #[cfg(unix)]
    #[test]
    fn planted_temporary_symlink_preserves_victim() {
        let directory = directory();
        let path = directory.join("writer-0.log");
        let victim = directory.join("victim");
        fs::write(&victim, b"victim bytes").unwrap();
        std::os::unix::fs::symlink(&victim, path.with_extension("tmp")).unwrap();
        replace(&path, b"replacement").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"victim bytes");
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn durable_constructor_preserves_planted_symlink_and_restarts() {
        use crate::local::DurableReplica;
        use crate::ownership::WriterConfig;
        let directory = directory();
        let store = directory.join("store");
        fs::create_dir(&store).unwrap();
        let victim = directory.join("victim");
        fs::write(&victim, b"victim bytes").unwrap();
        std::os::unix::fs::symlink(&victim, store.join("writer-0.tmp")).unwrap();
        let config = WriterConfig {
            writers: 2,
            writer: 0,
        };
        let mut replica = DurableReplica::counter(&store, config).unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"victim bytes");
        replica.bump(replica.ticket(), 5).unwrap();
        let state = replica.state().state().to_vec();
        drop(replica);
        let replica = DurableReplica::restart_counter(&store, config).unwrap();
        assert_eq!(replica.state().state(), state);
        drop(replica);
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn temporary_entry_types_and_refused_write_recovery() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = directory();
        let path = directory.join("writer-0.log");
        let temporary = path.with_extension("tmp");
        let missing = directory.join("missing");
        symlink(&missing, &temporary).unwrap();
        replace(&path, b"dangling").unwrap();
        assert!(!missing.exists());
        fs::remove_file(&temporary).unwrap();
        let target = directory.join("target-directory");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"keep").unwrap();
        symlink(&target, &temporary).unwrap();
        replace(&path, b"directory-link").unwrap();
        assert_eq!(fs::read(target.join("keep")).unwrap(), b"keep");
        fs::remove_file(&temporary).unwrap();
        fs::write(&temporary, b"stale crash bytes").unwrap();
        replace(&path, b"stale recovered").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"stale recovered");
        assert_eq!(fs::read(&temporary).unwrap(), b"stale crash bytes");
        fs::remove_file(&temporary).unwrap();
        fs::create_dir(&temporary).unwrap();
        replace(&path, b"tmp directory untouched").unwrap();
        assert!(temporary.is_dir());
        assert_eq!(fs::read(&path).unwrap(), b"tmp directory untouched");
        fs::remove_dir(&temporary).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        let refused = replace(&path, b"read-only");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        replace(&path, b"whole again").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"whole again");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exclusive_temporary_names_are_cleaned_at_all_error_checkpoints() {
        let directory = directory();
        let path = directory.join("writer-0.log");
        for boundary in 1..=6 {
            fs::write(&path, b"old").unwrap();
            FAULT.with(|fault| fault.set((boundary, false)));
            let result = replace(&path, b"replacement");
            FAULT.with(|fault| fault.set((0, false)));
            assert!(result.is_err());
            let entries: std::vec::Vec<_> = fs::read_dir(&directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(entries, std::vec![path.clone()], "checkpoint {boundary}");
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refused_write_then_remove_link_write_and_restart() {
        use crate::local::DurableReplica;
        use crate::ownership::WriterConfig;
        let directory = directory();
        let store = directory.join("store");
        let config = WriterConfig {
            writers: 2,
            writer: 0,
        };
        let mut replica = DurableReplica::counter(&store, config).unwrap();
        replica.bump(replica.ticket(), 5).unwrap();
        let temporary = store.join("writer-0.tmp");
        let victim = directory.join("victim");
        fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, &temporary).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&store, fs::Permissions::from_mode(0o555)).unwrap();
        let refused = replica.bump(replica.ticket(), 8);
        fs::set_permissions(&store, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(refused.is_err());
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        fs::remove_file(&temporary).unwrap();
        drop(replica);
        let mut replica = DurableReplica::restart_counter(&store, config).unwrap();
        replica.bump(replica.ticket(), 8).unwrap();
        let state = replica.state().state().to_vec();
        assert_eq!(state[0], 8);
        drop(replica);
        let replica = DurableReplica::restart_counter(&store, config).unwrap();
        assert_eq!(replica.state().state(), state);
        drop(replica);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn errors_leave_no_temporary_file() {
        let directory = directory();
        let mut orphans = std::vec::Vec::new();
        for boundary in 1..=6 {
            let path = directory.join(std::format!("checkpoint-{boundary}.log"));
            fs::write(&path, b"old").unwrap();
            FAULT.with(|fault| fault.set((boundary, false)));
            let result = replace(&path, b"replacement");
            FAULT.with(|fault| fault.set((0, false)));
            let error = result.unwrap_err();
            assert_eq!(error.to_string(), "injected uncertain I/O");
            let temporary = path.with_extension("tmp");
            let orphan = temporary.try_exists().unwrap();
            std::println!("checkpoint {boundary}: orphan={orphan}, path={temporary:?}");
            if orphan {
                orphans.push(boundary);
            }
            let expected: &[u8] = if boundary < 5 { b"old" } else { b"replacement" };
            assert_eq!(fs::read(&path).unwrap(), expected);
        }
        assert!(
            orphans.is_empty(),
            "orphan temporary files at checkpoints {orphans:?}"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rename_error_leaves_no_temporary_file() {
        let directory = directory();
        let path = directory.join("destination");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), b"old").unwrap();
        let error = replace(&path, b"replacement").unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"old");
        assert!(!path.with_extension("tmp").try_exists().unwrap());
        fs::remove_dir_all(directory).unwrap();
    }
}
