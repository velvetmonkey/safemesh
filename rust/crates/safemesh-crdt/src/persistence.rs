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
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
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
