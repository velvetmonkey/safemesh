use safemesh_crdt::{
    local::DurableReplica, ownership::WriterConfig, Admission, DecodeLimits,
    GCounter, GCounterDelta, Replica, ReplicaError, WireError,
};
use std::{env, fs, path::Path};

fn check(name: &str, result: Result<(), String>) -> bool {
    match result {
        Ok(()) => {
            println!("CHECK {name} PASS");
            true
        }
        Err(reason) => {
            println!("CHECK {name} FAIL {reason}");
            false
        }
    }
}

fn mutation() -> Result<(), String> {
    let mut counter = GCounter::new(2);
    counter.apply_bump(0, 3);
    counter.apply_bump(0, 7);
    if counter.state() != [7, 0] || counter.value() != 7 {
        return Err(format!("expected [7, 0] and 7, got {:?} and {}", counter.state(), counter.value()));
    }
    Ok(())
}

fn peers() -> Result<(Replica<GCounter>, Replica<GCounter>, Vec<u8>), String> {
    let mut left = Replica::new(GCounter::new(2));
    let mut right = Replica::new(GCounter::new(2));
    left.append(0, GCounterDelta { replica: 0, tally: 7 }).map_err(|e| e.to_string())?;
    right.append(1, GCounterDelta { replica: 1, tally: 4 }).map_err(|e| e.to_string())?;
    let left_bytes = left.log_bytes().map_err(|e| e.to_string())?;
    let right_bytes = right.log_bytes().map_err(|e| e.to_string())?;
    let left_admissions = left.merge_log_bytes(&right_bytes, DecodeLimits::default()).map_err(|e| e.to_string())?;
    let right_admissions = right.merge_log_bytes(&left_bytes, DecodeLimits::default()).map_err(|e| e.to_string())?;
    if left_admissions != [Admission::Accepted] || right_admissions != [Admission::Accepted]
        || left.state().state() != [7, 4] || right.state().state() != [7, 4]
        || left.state().value() != 11 || right.state().value() != 11 {
        return Err(format!("expected two [7, 4] states and value 11; got {:?}/{:?}", left.state().state(), right.state().state()));
    }
    Ok((left, right, left_bytes))
}

fn errors(mut left: Replica<GCounter>, bytes: &[u8]) -> Result<(), String> {
    let before = left.state().state().to_vec();
    let truncated = &bytes[..1];
    let short_error = left.merge_log_bytes(truncated, DecodeLimits::default());
    if short_error != Err(ReplicaError::LogDecode(safemesh_crdt::DecodeError::Wire(WireError::UnexpectedEof))) {
        return Err(format!("expected LogDecode(Wire(UnexpectedEof)), got {short_error:?}"));
    }
    let mut bad = bytes.to_vec();
    bad[0] = 0xff;
    let tag_error = left.merge_log_bytes(&bad, DecodeLimits::default());
    if tag_error != Err(ReplicaError::LogDecode(safemesh_crdt::DecodeError::Wire(WireError::InvalidTag))) {
        return Err(format!("expected LogDecode(Wire(InvalidTag)), got {tag_error:?}"));
    }
    if left.state().state() != before || left.state().value() != 11 {
        return Err(format!("error changed state to {:?}", left.state().state()));
    }
    Ok(())
}

fn recovery(root: &Path) -> Result<(), String> {
    let config = WriterConfig { writers: 2, writer: 0 };
    fs::create_dir(root).map_err(|e| e.to_string())?;
    {
        let mut store = DurableReplica::<GCounter>::counter(root, config).map_err(|e| e.to_string())?;
        store.bump(store.ticket(), 3).map_err(|e| e.to_string())?;
        store.bump(store.ticket(), 7).map_err(|e| e.to_string())?;
        if store.state().value() != 7 { return Err(format!("expected persisted 7, got {}", store.state().value())); }
    }
    let reopened = DurableReplica::<GCounter>::restart_counter(root, config).map_err(|e| e.to_string())?;
    if reopened.state().state() != [7, 0] || reopened.state().value() != 7 {
        return Err(format!("expected reopened [7, 0] and 7, got {:?} and {}", reopened.state().state(), reopened.state().value()));
    }
    Ok(())
}

fn main() {
    let root = env::args().nth(1).expect("pass a new store path");
    let c1 = check("MUTATION", mutation());
    let exchange = peers();
    let c2 = check("EXCHANGE", exchange.as_ref().map(|_| ()).map_err(Clone::clone));
    let c3 = match exchange {
        Ok((left, _, bytes)) => check("ERRORS", errors(left, &bytes)),
        Err(reason) => check("ERRORS", Err(format!("exchange failed: {reason}"))),
    };
    let c4 = check("RECOVERY", recovery(Path::new(&root)));
    if c1 && c2 && c3 && c4 {
        println!("PACKAGED RUST CONSUMER PASS checks=4");
    } else {
        std::process::exit(1);
    }
}
