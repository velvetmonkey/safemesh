// SafeMesh — canonical wire codec implementations.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! Canonical wire codec implementations for the CRDT types.

use alloc::string::String;
use alloc::vec::Vec;

use super::{
    read_tag, write_bytes, write_len, write_u64, write_u8, EnableWinsFlag, EnableWinsFlagDelta,
    EventLog, GCounterDelta, GSet, LwwMap, LwwMapDelta, LwwRegister, LwwRegisterDelta, OrSet,
    OrSetDelta, PnCounterDelta, Record, RecordId, Rga, WireCursor, WireDecode, WireEncode,
    WireError, WireSchema, TAG_ENABLE_WINS_FLAG_DISABLE_U64, TAG_ENABLE_WINS_FLAG_ENABLE_U64,
    TAG_ENABLE_WINS_FLAG_U64, TAG_GCOUNTER_DELTA, TAG_GSET_U64, TAG_LWW_MAP_REMOVE_U64,
    TAG_LWW_MAP_SET_U64, TAG_LWW_MAP_U64, TAG_LWW_REGISTER_DELTA_U64, TAG_LWW_REGISTER_U64,
    TAG_ORSET_ADD_STRING, TAG_ORSET_ADD_U64, TAG_ORSET_REMOVE_STRING, TAG_ORSET_REMOVE_U64,
    TAG_ORSET_STRING, TAG_ORSET_U64, TAG_PNCOUNTER_DEC, TAG_PNCOUNTER_INC, TAG_RECORD, TAG_RGA_U64,
};

impl WireEncode for GCounterDelta {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_GCOUNTER_DELTA);
        write_u64(
            out,
            u64::try_from(self.replica).map_err(|_| WireError::LengthOverflow)?,
        );
        write_u64(out, self.tally);
        Ok(())
    }
}

impl WireDecode for GCounterDelta {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_GCOUNTER_DELTA)?;
        let replica = usize::try_from(cursor.read_u64()?).map_err(|_| WireError::LengthOverflow)?;
        let tally = cursor.read_u64()?;
        Ok(GCounterDelta { replica, tally })
    }
}

impl WireEncode for PnCounterDelta {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            PnCounterDelta::Inc { replica, tally } => {
                write_u8(out, TAG_PNCOUNTER_INC);
                write_u64(
                    out,
                    u64::try_from(*replica).map_err(|_| WireError::LengthOverflow)?,
                );
                write_u64(out, *tally);
            }
            PnCounterDelta::Dec { replica, tally } => {
                write_u8(out, TAG_PNCOUNTER_DEC);
                write_u64(
                    out,
                    u64::try_from(*replica).map_err(|_| WireError::LengthOverflow)?,
                );
                write_u64(out, *tally);
            }
        }
        Ok(())
    }
}

impl WireDecode for PnCounterDelta {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let tag = cursor.read_u8()?;
        let replica = usize::try_from(cursor.read_u64()?).map_err(|_| WireError::LengthOverflow)?;
        let tally = cursor.read_u64()?;
        match tag {
            TAG_PNCOUNTER_INC => Ok(PnCounterDelta::Inc { replica, tally }),
            TAG_PNCOUNTER_DEC => Ok(PnCounterDelta::Dec { replica, tally }),
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for GSet<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_GSET_U64);
        write_len(out, self.elements.len())?;
        for element in &self.elements {
            write_u64(out, *element);
        }
        Ok(())
    }
}

impl WireDecode for GSet<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_GSET_U64)?;
        let mut set = GSet::new();
        for _ in 0..cursor.read_len()? {
            set.insert(cursor.read_u64()?);
        }
        Ok(set)
    }
}

impl WireEncode for OrSetDelta<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            OrSetDelta::Add { element, token } => {
                write_u8(out, TAG_ORSET_ADD_U64);
                write_u64(out, *element);
                write_u64(out, *token);
            }
            OrSetDelta::Remove { tokens } => {
                write_u8(out, TAG_ORSET_REMOVE_U64);
                write_len(out, tokens.len())?;
                // Preserve order and duplicates for exact delta round trips.
                for token in tokens {
                    write_u64(out, *token);
                }
            }
        }
        Ok(())
    }
}

impl WireDecode for OrSetDelta<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        match cursor.read_u8()? {
            TAG_ORSET_ADD_U64 => Ok(OrSetDelta::Add {
                element: cursor.read_u64()?,
                token: cursor.read_u64()?,
            }),
            TAG_ORSET_REMOVE_U64 => {
                let mut tokens = Vec::new();
                for _ in 0..cursor.read_len()? {
                    tokens.push(cursor.read_u64()?);
                }
                Ok(OrSetDelta::Remove { tokens })
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

// UTF-8 delta tags are distinct from the u64 delta tags, including Remove.
// Add carries a byte-length-prefixed UTF-8 element followed by a u64 token;
// Remove carries a token count followed by u64 tokens in their original order.
impl WireEncode for OrSetDelta<String, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            OrSetDelta::Add { element, token } => {
                write_u8(out, TAG_ORSET_ADD_STRING);
                write_bytes(out, element.as_bytes())?;
                write_u64(out, *token);
            }
            OrSetDelta::Remove { tokens } => {
                write_u8(out, TAG_ORSET_REMOVE_STRING);
                write_len(out, tokens.len())?;
                for token in tokens {
                    write_u64(out, *token);
                }
            }
        }
        Ok(())
    }
}

impl WireDecode for OrSetDelta<String, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        match cursor.read_u8()? {
            TAG_ORSET_ADD_STRING => {
                let len = cursor.read_len()?;
                let element = core::str::from_utf8(cursor.read_exact(len)?)
                    .map_err(|_| WireError::InvalidUtf8)?;
                let token = cursor.read_u64()?;
                Ok(OrSetDelta::Add {
                    element: String::from(element),
                    token,
                })
            }
            TAG_ORSET_REMOVE_STRING => {
                let mut tokens = Vec::new();
                for _ in 0..cursor.read_len()? {
                    tokens.push(cursor.read_u64()?);
                }
                Ok(OrSetDelta::Remove { tokens })
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for OrSet<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_ORSET_U64);
        write_len(out, self.adds.len())?;
        for (element, token) in &self.adds {
            write_u64(out, *element);
            write_u64(out, *token);
        }
        write_len(out, self.tombstones.len())?;
        for token in &self.tombstones {
            write_u64(out, *token);
        }
        Ok(())
    }
}

impl WireDecode for OrSet<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_ORSET_U64)?;
        let mut set = OrSet::new();
        for _ in 0..cursor.read_len()? {
            let element = cursor.read_u64()?;
            let token = cursor.read_u64()?;
            set.add(element, token);
        }
        let mut tombstones = Vec::new();
        for _ in 0..cursor.read_len()? {
            tombstones.push(cursor.read_u64()?);
        }
        set.apply_remove(tombstones);
        Ok(set)
    }
}

impl WireEncode for OrSet<String, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_ORSET_STRING);
        write_len(out, self.adds.len())?;
        for (element, token) in &self.adds {
            write_bytes(out, element.as_bytes())?;
            write_u64(out, *token);
        }
        write_len(out, self.tombstones.len())?;
        for token in &self.tombstones {
            write_u64(out, *token);
        }
        Ok(())
    }
}

impl WireDecode for OrSet<String, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_ORSET_STRING)?;
        let mut set = OrSet::new();
        for _ in 0..cursor.read_len()? {
            let len = cursor.read_len()?;
            let element = core::str::from_utf8(cursor.read_exact(len)?)
                .map_err(|_| WireError::InvalidUtf8)?;
            let element = String::from(element);
            let token = cursor.read_u64()?;
            if !set.adds.insert((element, token)) {
                return Err(WireError::DuplicateEntry);
            }
        }
        let mut tombstones = Vec::new();
        for _ in 0..cursor.read_len()? {
            tombstones.push(cursor.read_u64()?);
        }
        set.apply_remove(tombstones);
        Ok(set)
    }
}

impl WireEncode for Rga<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_RGA_U64);
        write_len(out, self.placed.len())?;
        for (position, value) in &self.placed {
            write_u64(out, *position);
            write_u64(out, *value);
        }
        write_len(out, self.tombstones.len())?;
        for position in &self.tombstones {
            write_u64(out, *position);
        }
        Ok(())
    }
}

impl WireDecode for Rga<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_RGA_U64)?;
        let mut rga = Rga::new();
        for _ in 0..cursor.read_len()? {
            let position = cursor.read_u64()?;
            let value = cursor.read_u64()?;
            rga.insert(position, value);
        }
        for _ in 0..cursor.read_len()? {
            rga.delete(cursor.read_u64()?);
        }
        Ok(rga)
    }
}

impl<D: WireEncode> WireEncode for Record<D> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_RECORD);
        write_u64(out, self.id.replica);
        write_u64(out, self.id.sequence);
        write_bytes(out, &self.delta.to_wire_bytes()?)?;
        Ok(())
    }
}

impl<D: WireDecode> WireDecode for Record<D> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_RECORD)?;
        let id = RecordId {
            replica: cursor.read_u64()?,
            sequence: cursor.read_u64()?,
        };
        let delta_len = cursor.read_len()?;
        let delta_bytes = cursor.read_exact(delta_len)?;
        let delta = D::from_wire_bytes(delta_bytes)?;
        Ok(Record { id, delta })
    }
}

// Shape-bearing frame: tag, body length, complemented length,
// body (shape marker, schema, arity, record count and length-prefixed records),
// CRC of length fields + body. Old unshaped frames return MissingShape.
// u32::MAX cannot be the count of a valid old body within a u32 frame length.
// Frame lengths, count and CRC are little-endian u32. Check the length pair before trusting it,
// then verify the CRC before decoding any record or invoking a payload decoder.
impl<D: WireEncode + WireSchema> WireEncode for EventLog<D> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        Self::encode_records(self.replica_count, &self.records, out)
    }
}

impl<D: WireDecode + WireSchema + PartialEq> WireDecode for EventLog<D> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        Self::decode_with(cursor, |_| {}, |_| Ok::<(), WireError>(()))
    }
}

impl WireEncode for LwwRegisterDelta<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_LWW_REGISTER_DELTA_U64);
        write_u64(out, self.timestamp);
        write_u64(out, self.replica);
        write_u64(out, self.value);
        Ok(())
    }
}

impl WireDecode for LwwRegisterDelta<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_LWW_REGISTER_DELTA_U64)?;
        Ok(LwwRegisterDelta {
            timestamp: cursor.read_u64()?,
            replica: cursor.read_u64()?,
            value: cursor.read_u64()?,
        })
    }
}

impl WireEncode for LwwRegister<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_LWW_REGISTER_U64);
        match self.entry() {
            Some(entry) => {
                write_u8(out, 1);
                write_u64(out, entry.dot.timestamp);
                write_u64(out, entry.dot.replica);
                write_u64(out, entry.value);
            }
            None => write_u8(out, 0),
        }
        Ok(())
    }
}

impl WireDecode for LwwRegister<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_LWW_REGISTER_U64)?;
        let present = cursor.read_u8()?;
        match present {
            0 => Ok(LwwRegister::new()),
            1 => {
                let mut register = LwwRegister::new();
                register.set(cursor.read_u64()?, cursor.read_u64()?, cursor.read_u64()?);
                Ok(register)
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for EnableWinsFlagDelta<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            EnableWinsFlagDelta::Enable { token } => {
                write_u8(out, TAG_ENABLE_WINS_FLAG_ENABLE_U64);
                write_u64(out, *token);
            }
            EnableWinsFlagDelta::Disable { tokens } => {
                write_u8(out, TAG_ENABLE_WINS_FLAG_DISABLE_U64);
                write_len(out, tokens.len())?;
                for token in tokens {
                    write_u64(out, *token);
                }
            }
        }
        Ok(())
    }
}

impl WireDecode for EnableWinsFlagDelta<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let tag = cursor.read_u8()?;
        match tag {
            TAG_ENABLE_WINS_FLAG_ENABLE_U64 => Ok(EnableWinsFlagDelta::Enable {
                token: cursor.read_u64()?,
            }),
            TAG_ENABLE_WINS_FLAG_DISABLE_U64 => {
                let len = cursor.read_len()?;
                let mut tokens = Vec::new();
                for _ in 0..len {
                    tokens.push(cursor.read_u64()?);
                }
                Ok(EnableWinsFlagDelta::Disable { tokens })
            }
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for EnableWinsFlag<u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_ENABLE_WINS_FLAG_U64);
        write_len(out, self.enables.len())?;
        for token in self.enables.iter() {
            write_u64(out, *token);
        }
        write_len(out, self.tombstones.len())?;
        for token in self.tombstones.iter() {
            write_u64(out, *token);
        }
        Ok(())
    }
}

impl WireDecode for EnableWinsFlag<u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_ENABLE_WINS_FLAG_U64)?;
        let enable_len = cursor.read_len()?;
        let mut flag = EnableWinsFlag::new();
        for _ in 0..enable_len {
            flag.enable(cursor.read_u64()?);
        }
        let tombstone_len = cursor.read_len()?;
        for _ in 0..tombstone_len {
            flag.disable([cursor.read_u64()?]);
        }
        Ok(flag)
    }
}

impl WireEncode for LwwMapDelta<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        match self {
            LwwMapDelta::Set {
                key,
                timestamp,
                replica,
                value,
            } => {
                write_u8(out, TAG_LWW_MAP_SET_U64);
                write_u64(out, *key);
                write_u64(out, *timestamp);
                write_u64(out, *replica);
                write_u64(out, *value);
            }
            LwwMapDelta::Remove {
                key,
                timestamp,
                replica,
            } => {
                write_u8(out, TAG_LWW_MAP_REMOVE_U64);
                write_u64(out, *key);
                write_u64(out, *timestamp);
                write_u64(out, *replica);
            }
        }
        Ok(())
    }
}

impl WireDecode for LwwMapDelta<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        let tag = cursor.read_u8()?;
        match tag {
            TAG_LWW_MAP_SET_U64 => Ok(LwwMapDelta::Set {
                key: cursor.read_u64()?,
                timestamp: cursor.read_u64()?,
                replica: cursor.read_u64()?,
                value: cursor.read_u64()?,
            }),
            TAG_LWW_MAP_REMOVE_U64 => Ok(LwwMapDelta::Remove {
                key: cursor.read_u64()?,
                timestamp: cursor.read_u64()?,
                replica: cursor.read_u64()?,
            }),
            _ => Err(WireError::InvalidTag),
        }
    }
}

impl WireEncode for LwwMap<u64, u64> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        write_u8(out, TAG_LWW_MAP_U64);
        write_len(out, self.entries.len())?;
        for (key, entry) in self.entries.iter() {
            write_u64(out, *key);
            write_u64(out, entry.dot.timestamp);
            write_u64(out, entry.dot.replica);
            write_u64(out, entry.value);
        }
        write_len(out, self.removals.len())?;
        for (key, dot) in self.removals.iter() {
            write_u64(out, *key);
            write_u64(out, dot.timestamp);
            write_u64(out, dot.replica);
        }
        Ok(())
    }
}

impl WireDecode for LwwMap<u64, u64> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        read_tag(cursor, TAG_LWW_MAP_U64)?;
        let entry_len = cursor.read_len()?;
        let mut map = LwwMap::new();
        for _ in 0..entry_len {
            map.set(
                cursor.read_u64()?,
                cursor.read_u64()?,
                cursor.read_u64()?,
                cursor.read_u64()?,
            );
        }
        let removal_len = cursor.read_len()?;
        for _ in 0..removal_len {
            map.remove(cursor.read_u64()?, cursor.read_u64()?, cursor.read_u64()?);
        }
        Ok(map)
    }
}
