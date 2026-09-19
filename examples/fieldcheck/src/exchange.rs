// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Fieldcheck-only framing and durable peer evidence; not an authenticated transport.
use crate::{canonical, records, Replica};
use safemesh_crdt::{
    ownership::allocate_token, Admission, OrSetDelta, Record, VersionVector, WireDecode, WireEncode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

type Event = Record<OrSetDelta<String, u64>>;
const MAX_FRAME: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    writer: u64,
    sequence: u64,
    bytes: Vec<u8>,
}
impl Entry {
    fn of(record: &Event) -> Result<Self, String> {
        Ok(Self {
            writer: record.id.replica,
            sequence: record.id.sequence,
            bytes: record.to_wire_bytes().map_err(|e| format!("{e:?}"))?,
        })
    }
    fn decode(&self) -> Result<Event, String> {
        let r = Event::from_wire_bytes(&self.bytes).map_err(|e| format!("{e:?}"))?;
        let OrSetDelta::Add { element, token } = &r.delta else {
            return Err("removal refused".into());
        };
        if self.writer > 1
            || self.sequence == 0
            || r.id.replica != self.writer
            || r.id.sequence != self.sequence
            || canonical(element)?.1 != *element
            || allocate_token(2, self.writer, self.sequence) != Some(*token)
            || Self::of(&r)? != *self
        {
            return Err("noncanonical record or invalid ownership".into());
        }
        Ok(r)
    }
}
fn full_map(replica: &Replica) -> Result<Vec<Entry>, String> {
    let mut entries = replica
        .log()
        .records()
        .iter()
        .map(Entry::of)
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| (e.writer, e.sequence));
    Ok(entries)
}
fn validate_map(entries: &[Entry]) -> Result<(), String> {
    let mut previous = None;
    let mut events = BTreeSet::new();
    for entry in entries {
        let key = (entry.writer, entry.sequence);
        if previous.is_some_and(|p| p >= key) {
            return Err("map must have sorted unique RecordIds".into());
        }
        previous = Some(key);
        let r = entry.decode()?;
        let OrSetDelta::Add { element, .. } = r.delta else {
            unreachable!()
        };
        if !events.insert(canonical(&element)?.0) {
            return Err("application identity collision".into());
        }
    }
    Ok(())
}
fn replace(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    let mut f = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())?;
    fs::File::open(path.parent().ok_or("no parent")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn barrier(name: &str) -> Result<(), String> {
    if let Ok(path) = std::env::var(name) {
        fs::write(&path, b"paused\n").map_err(|e| e.to_string())?;
        while Path::new(&path).exists() {
            thread::sleep(Duration::from_millis(20));
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    protocol: u32,
    peer: u64,
    records: Vec<Entry>,
}

pub struct Service {
    pub replica: Replica,
    pub writer: u64,
    root: PathBuf,
    receipt: Option<Receipt>,
    pub network: String,
    failed: bool,
}
impl Service {
    pub fn open(replica: Replica, writer: u64, root: &Path) -> Result<Self, String> {
        let mut service = Self {
            replica,
            writer,
            root: root.to_owned(),
            receipt: None,
            network: "Disconnected".into(),
            failed: false,
        };
        let pending = root.join("incoming.json");
        if pending.exists() {
            let entries: Vec<Entry> =
                serde_json::from_slice(&fs::read(&pending).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            service.accept(&entries, false)?;
        }
        let path = root.join("peer-receipt.json");
        match fs::read(path) {
            Ok(bytes) => {
                let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if receipt.protocol != 1 || receipt.peer != 1 - writer {
                    return Err("invalid receipt identity".into());
                }
                validate_map(&receipt.records)?;
                service.check_compatible(&receipt.records)?;
                service.receipt = Some(receipt);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
        records(&service.replica)?;
        Ok(service)
    }
    fn check_compatible(&self, entries: &[Entry]) -> Result<Vec<Event>, String> {
        let mut ids: BTreeMap<_, _> = full_map(&self.replica)?
            .into_iter()
            .map(|e| ((e.writer, e.sequence), e))
            .collect();
        let mut events = records(&self.replica)?;
        let mut decoded = Vec::new();
        for entry in entries {
            let r = entry.decode()?;
            let key = (entry.writer, entry.sequence);
            if let Some(old) = ids.get(&key) {
                if old != entry {
                    return Err("RecordId collision".into());
                }
            } else {
                let OrSetDelta::Add { element, .. } = &r.delta else {
                    unreachable!()
                };
                let (id, _) = canonical(element)?;
                if events.insert(id, Value::Null).is_some() {
                    return Err("application identity collision".into());
                }
                ids.insert(key, entry.clone());
            }
            decoded.push(r);
        }
        Ok(decoded)
    }
    fn accept(&mut self, entries: &[Entry], pause: bool) -> Result<(), String> {
        if self.failed {
            return Err("storage failed; restart required".into());
        }
        let decoded = self.check_compatible(entries)?;
        if entries.is_empty() {
            return Ok(());
        }
        if pause {
            barrier("FIELDCHECK_BEFORE_ACCEPT")?;
        }
        // Persist the accepted incoming batch BEFORE receive. Replay this journal on
        // restart. receive itself durably commits each record before changing state.
        self.failed = true;
        replace(&self.root.join("incoming.json"), &entries)?;
        for record in decoded {
            match self
                .replica
                .receive(self.replica.ticket(), record)
                .map_err(|e| format!("{e:?}"))?
            {
                Admission::Accepted | Admission::Duplicate => (),
                other => return Err(format!("admission refused: {other:?}")),
            }
        }
        fs::remove_file(self.root.join("incoming.json")).map_err(|e| e.to_string())?;
        fs::File::open(&self.root)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        self.failed = false;
        Ok(())
    }
    fn confirm(&mut self, peer: u64, entries: Vec<Entry>) -> Result<(), String> {
        if self.failed || peer != 1 - self.writer {
            return Err("receipt refused".into());
        }
        validate_map(&entries)?;
        self.check_compatible(&entries)?;
        if self.receipt.as_ref().is_some_and(|r| r.records == entries) {
            return Ok(());
        }
        barrier("FIELDCHECK_BEFORE_RECEIPT")?;
        let receipt = Receipt {
            protocol: 1,
            peer,
            records: entries,
        };
        self.failed = true;
        replace(&self.root.join("peer-receipt.json"), &receipt)?;
        self.receipt = Some(receipt);
        self.failed = false;
        Ok(())
    }
    pub fn writable(&self) -> bool {
        !self.failed
    }
    pub fn status(&self) -> Result<Value, String> {
        let local = full_map(&self.replica)?;
        let agreed = !self.failed && self.receipt.as_ref().is_some_and(|r| r.records == local);
        let confirmed = self.receipt.as_ref().map_or(0, |r| {
            local.iter().filter(|e| r.records.contains(e)).count()
        });
        Ok(
            json!({"status":true, "records":records(&self.replica)?.values().collect::<Vec<_>>(),
            "record_map":local, "projected":self.replica.state().elements().len(),
            "peer_confirmed":confirmed,"agreed":agreed,"network":self.network,
            "delivery":if agreed { format!("Both devices have these {} records (last durable peer confirmation)", local.len()) } else { "Waiting for durable peer confirmation of this exact set".into() }}),
        )
    }
    fn summary(&self) -> Message {
        Message::Summary {
            protocol: 1,
            writer: self.writer,
            prefixes: self
                .replica
                .log()
                .version()
                .entries()
                .iter()
                .map(|(&w, &s)| (w, s))
                .collect(),
            zeros: self.replica.log().version().zero_replicas().clone(),
        }
    }
    fn batch(&self, summary: Message) -> Result<(u64, Message), String> {
        let Message::Summary {
            protocol: 1,
            writer,
            prefixes,
            zeros,
        } = summary
        else {
            return Err("expected version summary v1".into());
        };
        if writer != 1 - self.writer
            || prefixes.len() > 2
            || prefixes.iter().any(|(w, _)| *w > 1)
            || prefixes.windows(2).any(|p| p[0].0 >= p[1].0)
            || !zeros.is_empty()
        {
            return Err("invalid peer or version domain".into());
        }
        let version = VersionVector::from_peer_prefixes(&prefixes.into_iter().collect(), &zeros)
            .map_err(|e| e.to_string())?;
        let entries = self
            .replica
            .log()
            .since(&version)
            .iter()
            .map(Entry::of)
            .collect::<Result<_, _>>()?;
        Ok((writer, Message::Batch { records: entries }))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Message {
    Summary {
        protocol: u32,
        writer: u64,
        prefixes: Vec<(u64, u64)>,
        zeros: BTreeSet<u64>,
    },
    Batch {
        records: Vec<Entry>,
    },
    Receipt {
        writer: u64,
        records: Vec<Entry>,
    },
}
// CRC detects accidental byte changes; neither CRC nor the full-map comparison
// authenticates a malicious peer. This fixed pair is for a trusted loopback link.
fn crc(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= *b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}
fn send(stream: &mut TcpStream, message: &Message) -> Result<(), String> {
    let bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FRAME {
        return Err("frame exceeds 16 MiB".into());
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| stream.write_all(&crc(&bytes).to_be_bytes()))
        .and_then(|_| stream.write_all(&bytes))
        .map_err(|e| e.to_string())
}
fn read(stream: &mut TcpStream) -> Result<Message, String> {
    let mut header = [0; 8];
    stream.read_exact(&mut header).map_err(|e| e.to_string())?;
    let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err("invalid frame length".into());
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    if crc(&bytes) != u32::from_be_bytes(header[4..].try_into().unwrap()) {
        return Err("frame integrity mismatch".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
type Shared = Arc<Mutex<Service>>;
fn receive_batch(stream: &mut TcpStream, state: &Shared) -> Result<(), String> {
    let Message::Batch { records } = read(stream)? else {
        return Err("expected batch".into());
    };
    let reply = {
        let mut state = state.lock().map_err(|e| e.to_string())?;
        state.accept(&records, true)?;
        Message::Receipt {
            writer: state.writer,
            records: full_map(&state.replica)?,
        }
    };
    send(stream, &reply)
}
fn receive_receipt(stream: &mut TcpStream, state: &Shared, peer: u64) -> Result<(), String> {
    let Message::Receipt { writer, records } = read(stream)? else {
        return Err("expected durable receipt".into());
    };
    if peer != writer {
        return Err("receipt from wrong peer".into());
    }
    state
        .lock()
        .map_err(|e| e.to_string())?
        .confirm(writer, records)
}
fn connection(mut stream: TcpStream, state: &Shared, initiator: bool) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    loop {
        // Ordered half-rounds avoid both peers filling their socket send buffers.
        if initiator {
            let summary = state.lock().map_err(|e| e.to_string())?.summary();
            send(&mut stream, &summary)?;
            let summary = read(&mut stream)?;
            let (peer, batch) = state.lock().map_err(|e| e.to_string())?.batch(summary)?;
            send(&mut stream, &batch)?;
            receive_receipt(&mut stream, state, peer)?;
            receive_batch(&mut stream, state)?;
        } else {
            let summary = read(&mut stream)?;
            let (peer, batch, own) = {
                let state = state.lock().map_err(|e| e.to_string())?;
                let (peer, batch) = state.batch(summary)?;
                (peer, batch, state.summary())
            };
            send(&mut stream, &own)?;
            receive_batch(&mut stream, state)?;
            send(&mut stream, &batch)?;
            receive_receipt(&mut stream, state, peer)?;
        }
        state.lock().map_err(|e| e.to_string())?.network = "Connected".into();
        if initiator {
            thread::sleep(Duration::from_millis(250));
        }
    }
}
pub fn start(state: Shared, listen: Option<&str>, connect: Option<&str>) -> Result<(), String> {
    if let Some(address) = listen {
        let listener = TcpListener::bind(address).map_err(|e| e.to_string())?;
        if !listener
            .local_addr()
            .map_err(|e| e.to_string())?
            .ip()
            .is_loopback()
        {
            return Err("Slice 2 requires loopback".into());
        }
        thread::spawn(move || {
            for stream in listener.incoming() {
                let outcome = stream
                    .map_err(|e| e.to_string())
                    .and_then(|s| connection(s, &state, false));
                state.lock().unwrap().network = format!("Disconnected: {}", outcome.unwrap_err());
            }
        });
    } else if let Some(address) = connect {
        let address: std::net::SocketAddr = address.parse().map_err(|_| "expected IP:port")?;
        if !address.ip().is_loopback() {
            return Err("Slice 2 requires loopback".into());
        }
        thread::spawn(move || loop {
            let outcome = TcpStream::connect_timeout(&address, Duration::from_secs(2))
                .map_err(|e| e.to_string())
                .and_then(|s| connection(s, &state, true));
            state.lock().unwrap().network = format!("Disconnected: {}", outcome.unwrap_err());
            thread::sleep(Duration::from_secs(1));
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn populated_summary_survives_tagged_json() {
        let message = Message::Summary {
            protocol: 1,
            writer: 1,
            prefixes: vec![(0, 8), (1, 2)],
            zeros: BTreeSet::new(),
        };
        let bytes = serde_json::to_vec(&message).unwrap();
        let decoded: Message = serde_json::from_slice(&bytes).unwrap();
        let Message::Summary { prefixes, .. } = decoded else {
            panic!("wrong message")
        };
        let version =
            VersionVector::from_peer_prefixes(&prefixes.into_iter().collect(), &BTreeSet::new())
                .unwrap();
        assert_eq!((version.get(0), version.get(1)), (8, 2));
    }

    #[test]
    fn crc_known_vector_and_single_byte_corruption() {
        assert_eq!(crc(b"123456789"), 0xcbf43926);
        assert_ne!(crc(b"123456789"), crc(b"123456788"));
    }
}
