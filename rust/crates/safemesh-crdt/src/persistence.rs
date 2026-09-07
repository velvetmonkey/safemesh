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
    fs::rename(&temporary, path)?;
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
