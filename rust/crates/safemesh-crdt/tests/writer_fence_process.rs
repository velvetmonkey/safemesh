#![cfg(all(target_os = "linux", feature = "local-writer"))]

use safemesh_crdt::local::{DurableReplica, LocalError};
use safemesh_crdt::ownership::WriterConfig;
use std::{env, fs, path::Path, process::Command};

fn config() -> WriterConfig {
    WriterConfig {
        writers: 2,
        writer: 0,
    }
}

fn contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn second_process_cannot_write_same_writer() {
    if let Ok(root) = env::var("SAFEMESH_FENCE_CHILD_ROOT") {
        let result = DurableReplica::restart_counter(Path::new(&root), config());
        assert!(
            matches!(result, Err(LocalError::Refused)),
            "second process must get Refused"
        );
        return;
    }
    let root = env::temp_dir().join(format!("rust-writer-fence-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let store = root.join("store");
    let holder = DurableReplica::counter(&store, config()).unwrap();
    let before = contents(&store);
    let child = Command::new(env::current_exe().unwrap())
        .arg("--exact")
        .arg("second_process_cannot_write_same_writer")
        .arg("--nocapture")
        .env("SAFEMESH_FENCE_CHILD_ROOT", &store)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "child: {}",
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(
        contents(&store),
        before,
        "refused child changed store bytes"
    );
    drop(holder);
    let restarted = DurableReplica::restart_counter(&store, config()).unwrap();
    drop(restarted);
    fs::remove_dir_all(root).unwrap();
}
