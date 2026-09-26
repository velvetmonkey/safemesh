// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{
    Admission, Crdt, EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounterDelta, GSet, LwwMap,
    LwwMapDelta, LwwRegister, LwwRegisterDelta, OrSet, OrSetDelta, PnCounterDelta, Record, Rga,
    RgaDelta, VersionVector,
};
use alloc::{borrow::Cow, string::String, vec::Vec};

pub(super) const TAG_RECORD: u8 = 0x01;
/// Unchecked EventLog frame written before the CRC envelope; decoded only by migration.
pub(super) const TAG_EVENT_LOG_LEGACY: u8 = 0x02;
pub(super) const TAG_EVENT_LOG: u8 = 0x03;
pub(super) const TAG_VERSION_VECTOR: u8 = 0x04;
pub(super) const TAG_GCOUNTER_DELTA: u8 = 0x10;
pub(super) const TAG_PNCOUNTER_INC: u8 = 0x11;
pub(super) const TAG_PNCOUNTER_DEC: u8 = 0x12;
pub(super) const TAG_GSET_U64: u8 = 0x20;
pub(super) const TAG_ORSET_U64: u8 = 0x30;
pub(super) const TAG_ORSET_STRING: u8 = 0x35;
pub(super) const TAG_ORSET_ADD_U64: u8 = 0x31;
pub(super) const TAG_ORSET_REMOVE_U64: u8 = 0x32;
pub(super) const TAG_ORSET_ADD_STRING: u8 = 0x33;
pub(super) const TAG_ORSET_REMOVE_STRING: u8 = 0x34;
pub(super) const TAG_RGA_U64: u8 = 0x40;
pub(super) const TAG_RGA_INSERT_U64: u8 = 0x41;
pub(super) const TAG_RGA_DELETE_U64: u8 = 0x42;
pub(super) const TAG_LWW_REGISTER_DELTA_U64: u8 = 0x50;
pub(super) const TAG_LWW_REGISTER_U64: u8 = 0x51;
pub(super) const TAG_ENABLE_WINS_FLAG_ENABLE_U64: u8 = 0x60;
pub(super) const TAG_ENABLE_WINS_FLAG_DISABLE_U64: u8 = 0x61;
pub(super) const TAG_ENABLE_WINS_FLAG_U64: u8 = 0x62;
pub(super) const TAG_LWW_MAP_SET_U64: u8 = 0x70;
pub(super) const TAG_LWW_MAP_REMOVE_U64: u8 = 0x71;
pub(super) const TAG_LWW_MAP_U64: u8 = 0x72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WireError {
    OwnershipViolation,
    /// An OR-Set add record at sequence 0. An add mints a token at its record
    /// ID, and the Lean ownership rule allocates tokens only at positive
    /// sequences (`RecordKernel.allocateToken`, `RecordKernel.permitted`).
    ZeroSequenceAdd {
        /// Author of the refused record. Its sequence is 0.
        replica: u64,
    },
    UnexpectedEof,
    InvalidTag,
    TrailingBytes,
    LengthOverflow,
    RecordCollision,
    DuplicateEntry,
    IntegrityMismatch,
    InvalidUtf8,
    /// A fixed-domain log constructed or saved without its arity.
    MissingShape,
    /// A valid EventLog frame from an earlier encoder. Nothing was decoded;
    /// migrate it once with [`EventLog::migrate_legacy_wire_bytes_for`].
    LegacyEventLogFrame {
        found: LegacyFrame,
    },
    DeltaTypeMismatch,
    ReplicaCountMismatch {
        expected: usize,
        actual: usize,
    },
    /// Fixed and unbounded replica domains are incompatible.
    ArityKindMismatch,
    VersionAuthorLimitExceeded {
        max_authors: usize,
    },
    VersionZeroReplicaLimitExceeded {
        max_zero_replicas: usize,
    },
    CollectionElementLimitExceeded {
        max_elements: usize,
    },
    NestedRecordLimitExceeded {
        max_records: usize,
    },
    NonCanonicalVersionVector,
}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OwnershipViolation => {
                f.write_str("record violates replica ownership or coordinate bounds")
            }
            Self::ZeroSequenceAdd { replica } => write!(
                f,
                "OR-Set add record (replica {replica}, sequence 0) refused: add sequences start at 1. \
                 Recovery: re-add the element from replica {replica} at a positive sequence, \
                 and remove this record from any stored log before loading it again"
            ),
            Self::UnexpectedEof => f.write_str("unexpected end of wire input"),
            Self::InvalidTag => f.write_str("unexpected wire tag"),
            Self::TrailingBytes => f.write_str("unexpected trailing bytes after wire value"),
            Self::LengthOverflow => f.write_str("wire length exceeds the representable range"),
            Self::RecordCollision => f.write_str("record identity has conflicting payloads"),
            Self::DuplicateEntry => {
                f.write_str("wire OR-set contains a duplicate element and token")
            }
            Self::IntegrityMismatch => f.write_str("wire frame integrity check failed"),
            Self::InvalidUtf8 => f.write_str("wire string contains invalid UTF-8"),
            Self::MissingShape => f.write_str("wire frame is missing required shape metadata"),
            Self::LegacyEventLogFrame { found } => write!(
                f,
                "legacy EventLog frame: found {found}, expected tag 0x03 with shape header; \
                 migrate once with EventLog::migrate_legacy_wire_bytes_for(bytes, &destination) \
                 or `cargo run -p safemesh-crdt --example migrate_event_log`, giving the \
                 original replica count (safemesh-crdt README, \"Migrating a legacy EventLog\")"
            ),
            Self::DeltaTypeMismatch => {
                f.write_str("wire delta schema does not match the expected type")
            }
            Self::ReplicaCountMismatch { expected, actual } => write!(
                f,
                "replica-count mismatch: expected={expected}, actual={actual}"
            ),
            Self::ArityKindMismatch => {
                f.write_str("invalid or incompatible replica-domain arity kind")
            }
            Self::VersionAuthorLimitExceeded { max_authors } => {
                write!(f, "wire version exceeds author limit {max_authors}")
            }
            Self::VersionZeroReplicaLimitExceeded { max_zero_replicas } => {
                write!(
                    f,
                    "wire version exceeds zero-acknowledgement limit {max_zero_replicas}"
                )
            }
            Self::CollectionElementLimitExceeded { max_elements } => {
                write!(f, "wire collection exceeds element limit {max_elements}")
            }
            Self::NestedRecordLimitExceeded { max_records } => {
                write!(f, "nested wire log exceeds record limit {max_records}")
            }
            Self::NonCanonicalVersionVector => f.write_str("noncanonical wire version vector"),
        }
    }
}

impl core::error::Error for WireError {}

/// An EventLog frame layout written by an earlier encoder, named by
/// [`WireError::LegacyEventLogFrame`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyFrame {
    /// Tag `0x02`: record count and records, with no length pair, CRC or shape.
    Tag02,
    /// Tag `0x03` with the CRC envelope but no shape header (schema and arity).
    Tag03Unshaped,
}

impl core::fmt::Display for LegacyFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Tag02 => "tag 0x02 (no CRC, no shape header)",
            Self::Tag03Unshaped => "tag 0x03 without shape header",
        })
    }
}

/// Optional count budget for each built-in collection and collection delta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollectionLimits {
    /// Maximum declared count per collection. `None` removes the ceiling.
    pub max_elements: Option<usize>,
}

impl CollectionLimits {
    /// Default collection wire ceiling; callers may explicitly raise it.
    pub const WIRE_DEFAULT: Self = Self {
        max_elements: Some(4096),
    };
}

/// Optional limits for [`EventLog::from_wire_bytes_with_limits`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Maximum record occurrences in each EventLog frame, including nested
    /// frames and duplicates. `None` is unbounded; `Some(0)` admits only empty logs.
    /// This does not bound bytes or collection entries in a payload.
    pub max_records: Option<usize>,
    /// Maximum elements in each nested built-in collection. `None` uses
    /// the default ceiling of 4,096; use `Some(n)` to set an explicit ceiling.
    pub max_collection_elements: Option<usize>,
}

/// Failure from the opt-in bounded event-log decoder.
/// Separate from [`WireError`] to preserve existing exhaustive matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Wire(WireError),
    /// Stopped before decoding the next record; no partial log is returned.
    RecordLimitExceeded {
        max_records: usize,
    },
}

impl From<WireError> for DecodeError {
    fn from(error: WireError) -> Self {
        Self::Wire(error)
    }
}

/// Stable, versioned identity for persisted payloads, independent of Rust names.
/// External implementations must use a globally unique schema and change it when
/// the wire interpretation changes. Never reuse a built-in `safemesh/` identity.
pub trait WireSchema {
    /// Borrow fixed identities; return owned bytes for composed schemas.
    fn wire_schema() -> Cow<'static, [u8]>;
    const REQUIRES_ARITY: bool = false;
}

macro_rules! wire_schema {
    ($ty:ty, $name:literal, $fixed:expr) => {
        impl WireSchema for $ty {
            fn wire_schema() -> Cow<'static, [u8]> {
                Cow::Borrowed($name.as_bytes())
            }
            const REQUIRES_ARITY: bool = $fixed;
        }
    };
}

wire_schema!(GCounterDelta, "safemesh/gcounter-delta/v1", true);
wire_schema!(PnCounterDelta, "safemesh/pncounter-delta/v1", true);
wire_schema!(GSet<u64>, "safemesh/gset-u64/v1", false);
wire_schema!(OrSetDelta<u64, u64>, "safemesh/orset-delta-u64-u64/v1", false);
wire_schema!(OrSetDelta<String, u64>, "safemesh/orset-delta-utf8-u64/v1", false);
wire_schema!(OrSet<u64, u64>, "safemesh/orset-u64-u64/v1", false);
wire_schema!(OrSet<String, u64>, "safemesh/orset-utf8-u64/v1", false);
wire_schema!(Rga<u64, u64>, "safemesh/rga-u64-u64/v1", false);
wire_schema!(RgaDelta<u64, u64>, "safemesh/rga-delta-u64-u64/v1", false);
wire_schema!(VersionVector, "safemesh/version-vector/v1", false);
wire_schema!(
    LwwRegisterDelta<u64>,
    "safemesh/lww-register-delta-u64/v1",
    false
);
wire_schema!(LwwRegister<u64>, "safemesh/lww-register-u64/v1", false);
wire_schema!(
    EnableWinsFlagDelta<u64>,
    "safemesh/enable-wins-flag-delta-u64/v1",
    false
);
wire_schema!(
    EnableWinsFlag<u64>,
    "safemesh/enable-wins-flag-u64/v1",
    false
);
wire_schema!(LwwMapDelta<u64, u64>, "safemesh/lww-map-delta-u64-u64/v1", false);
wire_schema!(LwwMap<u64, u64>, "safemesh/lww-map-u64-u64/v1", false);

impl<D: WireSchema> WireSchema for Record<D> {
    fn wire_schema() -> Cow<'static, [u8]> {
        let mut schema = b"safemesh/record/v1/".to_vec();
        schema.extend_from_slice(D::wire_schema().as_ref());
        Cow::Owned(schema)
    }
}

impl<D: WireSchema> WireSchema for EventLog<D> {
    fn wire_schema() -> Cow<'static, [u8]> {
        let mut schema = b"safemesh/event-log/v2/".to_vec();
        schema.extend_from_slice(D::wire_schema().as_ref());
        Cow::Owned(schema)
    }
}

impl<D: WireDecode + WireSchema + PartialEq> EventLog<D> {
    /// Decode an inert log with an optional top-level record budget.
    ///
    /// Checks frame integrity and shape first, then stops before reading record
    /// `max_records + 1`, counting duplicate occurrences before deduplication.
    /// Returns [`DecodeError::RecordLimitExceeded`] without a partial result.
    /// CRC verification still scans the entire frame; callers must separately
    /// cap input bytes and payload complexity.
    /// As with [`WireDecode::from_wire_bytes`], this does not validate a destination
    /// CRDT for replay. The option is currently exposed only in Rust.
    ///
    /// [`DecodeLimits::default`] preserves the unbounded decoder's values and
    /// wire errors (wrapped in [`DecodeError::Wire`]).
    ///
    /// ```
    /// use safemesh_crdt::{DecodeError, DecodeLimits, EventLog, GSet, Record, RecordId};
    /// let records = [Record { id: RecordId { replica: 0, sequence: 1 }, delta: GSet::<u64>::new() }];
    /// let mut bytes = Vec::new();
    /// EventLog::encode_records(None, &records, &mut bytes).unwrap();
    /// assert_eq!(EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
    ///     &bytes, DecodeLimits { max_records: Some(0), ..DecodeLimits::default() }),
    ///     Err(DecodeError::RecordLimitExceeded { max_records: 0 }));
    /// let log = EventLog::<GSet<u64>>::from_wire_bytes_with_limits(
    ///     &bytes, DecodeLimits { max_records: Some(1), max_collection_elements: Some(5_000) }).unwrap();
    /// assert_eq!(log.records().len(), 1);
    /// ```
    pub fn from_wire_bytes_with_limits(
        bytes: &[u8],
        limits: DecodeLimits,
    ) -> Result<Self, DecodeError> {
        let mut cursor = WireCursor::new(bytes);
        let log = Self::decode_with(
            &mut cursor,
            CollectionLimits {
                max_elements: limits
                    .max_collection_elements
                    .or(CollectionLimits::WIRE_DEFAULT.max_elements),
            },
            limits.max_records,
            |_| {},
            |index| {
                if let Some(max_records) = limits.max_records {
                    if index >= max_records {
                        return Err(DecodeError::RecordLimitExceeded { max_records });
                    }
                }
                Ok(())
            },
        )?;
        if !cursor.is_empty() {
            return Err(WireError::TrailingBytes.into());
        }
        Ok(log)
    }

    /// Decode and compare the saved shape with the destination before replay.
    /// Plain `from_wire_bytes` decodes a log and retains its domain; it does not
    /// load a CRDT. Use this method at every persisted-state loading boundary.
    pub fn from_wire_bytes_for<C: Crdt<Delta = D>>(
        bytes: &[u8],
        state: &C,
    ) -> Result<Self, WireError> {
        let log = Self::from_wire_bytes(bytes)?;
        log.validate_for(state)?;
        Ok(log)
    }

    /// Decode with record and nested collection budgets, then validate the
    /// saved shape and records against the destination before replay.
    pub fn from_wire_bytes_for_with_limits<C: Crdt<Delta = D>>(
        bytes: &[u8],
        state: &C,
        limits: DecodeLimits,
    ) -> Result<Self, DecodeError> {
        let log = Self::from_wire_bytes_with_limits(bytes, limits)?;
        log.validate_for(state)?;
        Ok(log)
    }

    /// Decode all input occurrences in order for per-record admission reporting.
    /// Validate the entire frame, collisions and destination shape before returning.
    /// The ordinary log loaders continue to deduplicate identical records.
    pub fn records_from_wire_bytes_for<C: Crdt<Delta = D>>(
        bytes: &[u8],
        state: &C,
    ) -> Result<Vec<Record<D>>, WireError>
    where
        D: Clone,
    {
        let mut records = Vec::new();
        let mut cursor = WireCursor::new(bytes);
        let log = Self::decode_with(
            &mut cursor,
            CollectionLimits::WIRE_DEFAULT,
            None,
            |record| records.push(record.clone()),
            |_| Ok::<(), WireError>(()),
        )?;
        if !cursor.is_empty() {
            return Err(WireError::TrailingBytes);
        }
        log.validate_for(state)?;
        Ok(records)
    }

    /// Decode each occurrence with record and nested collection budgets, then
    /// validate the destination before returning the occurrences.
    pub fn records_from_wire_bytes_for_with_limits<C: Crdt<Delta = D>>(
        bytes: &[u8],
        state: &C,
        limits: DecodeLimits,
    ) -> Result<Vec<Record<D>>, DecodeError>
    where
        D: Clone,
    {
        let mut records = Vec::new();
        let mut cursor = WireCursor::new(bytes);
        let log = Self::decode_with(
            &mut cursor,
            CollectionLimits {
                max_elements: limits
                    .max_collection_elements
                    .or(CollectionLimits::WIRE_DEFAULT.max_elements),
            },
            limits.max_records,
            |record| records.push(record.clone()),
            |index| {
                if let Some(max_records) = limits.max_records {
                    if index >= max_records {
                        return Err(DecodeError::RecordLimitExceeded { max_records });
                    }
                }
                Ok(())
            },
        )?;
        if !cursor.is_empty() {
            return Err(WireError::TrailingBytes.into());
        }
        log.validate_for(state)?;
        Ok(records)
    }

    fn validate_for<C: Crdt<Delta = D>>(&self, state: &C) -> Result<(), WireError> {
        self.validate_shape(state)?;
        for record in self.records() {
            state.validate_record(record.id, &record.delta)?;
        }
        Ok(())
    }
}

impl<D: WireDecode + WireEncode + WireSchema + PartialEq> EventLog<D> {
    /// Re-encode a legacy EventLog frame ([`LegacyFrame`]) as a current shaped
    /// frame. This is explicit; no loader ever migrates automatically.
    ///
    /// Old frames record neither schema nor arity. The caller declares both:
    /// the schema by choosing `D`, and the arity through `state`, a destination
    /// carrier with the original replica count, verified independently. Do not
    /// infer it from the largest coordinate in the log. Every record is decoded
    /// and deduplicated as the old decoder did, then validated against `state`.
    /// Record bytes and order are unchanged; only the frame gains its shape. Any
    /// error returns no output. A current frame is validated the same way and
    /// returned unchanged, so a repeated migration has no further effect.
    /// Nested EventLog payloads are not rewritten; a legacy nested frame fails.
    ///
    /// ```
    /// use safemesh_crdt::{EventLog, GCounter, GCounterDelta, LegacyFrame, WireDecode, WireError};
    /// let old = [0x02, 0, 0, 0, 0]; // an empty tag 0x02 log
    /// assert_eq!(EventLog::<GCounterDelta>::from_wire_bytes(&old),
    ///     Err(WireError::LegacyEventLogFrame { found: LegacyFrame::Tag02 }));
    /// let state = GCounter::new(2);
    /// let new = EventLog::migrate_legacy_wire_bytes_for(&old, &state)?;
    /// assert_eq!(EventLog::from_wire_bytes_for(&new, &state)?.replica_count(), Some(2));
    /// # Ok::<(), WireError>(())
    /// ```
    pub fn migrate_legacy_wire_bytes_for<C: Crdt<Delta = D>>(
        bytes: &[u8],
        state: &C,
    ) -> Result<Vec<u8>, WireError> {
        match Self::migrate_legacy_wire_bytes_for_with_limits(bytes, state, DecodeLimits::default())
        {
            Ok(bytes) => Ok(bytes),
            Err(DecodeError::Wire(error)) => Err(error),
            Err(DecodeError::RecordLimitExceeded { .. }) => unreachable!("unbounded migration"),
        }
    }

    /// Migrate a legacy frame with a budget for each input record occurrence.
    /// Duplicate records count; the limit is checked before decoding the next record.
    /// A current shaped frame uses the same limited decoder as ordinary loading.
    pub fn migrate_legacy_wire_bytes_for_with_limits<C: Crdt<Delta = D>>(
        bytes: &[u8],
        state: &C,
        limits: DecodeLimits,
    ) -> Result<Vec<u8>, DecodeError> {
        let mut cursor = WireCursor::new(bytes);
        let mut body = match cursor.read_u8()? {
            TAG_EVENT_LOG_LEGACY => cursor,
            TAG_EVENT_LOG => {
                let body = read_checked_body(&mut cursor)?;
                if !cursor.is_empty() {
                    return Err(WireError::TrailingBytes.into());
                }
                if WireCursor::new(body).read_u32()? == u32::MAX {
                    Self::from_wire_bytes_for_with_limits(bytes, state, limits)?;
                    return Ok(bytes.to_vec());
                }
                WireCursor::new(body)
            }
            _ => return Err(WireError::InvalidTag.into()),
        };
        let mut log = EventLog::for_crdt(state);
        for index in 0..body.read_len()? {
            if let Some(max_records) = limits.max_records {
                if index >= max_records {
                    return Err(DecodeError::RecordLimitExceeded { max_records });
                }
            }
            let record_len = body.read_len()?;
            let mut record_cursor = WireCursor::new(body.read_exact(record_len)?);
            let record = Record::<D>::decode_wire_with_limits(
                &mut record_cursor,
                CollectionLimits {
                    max_elements: limits
                        .max_collection_elements
                        .or(CollectionLimits::WIRE_DEFAULT.max_elements),
                },
                limits.max_records,
            )?;
            if !record_cursor.is_empty() {
                return Err(WireError::TrailingBytes.into());
            }
            match log.identity_admission(&record) {
                Admission::Collision => return Err(WireError::RecordCollision.into()),
                Admission::Accepted => log.commit_record(record),
                Admission::Duplicate => {}
                Admission::Invalid(_) => unreachable!("identity check does not validate a carrier"),
            }
        }
        if !body.is_empty() {
            return Err(WireError::TrailingBytes.into());
        }
        log.validate_for(state)?;
        Ok(log.to_wire_bytes()?)
    }
}

/// Encode a payload using the canonical wire primitives.
///
/// Custom payloads choose their own layout and, for persistence, a unique [`WireSchema`].
pub trait WireEncode {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError>;

    fn to_wire_bytes(&self) -> Result<Vec<u8>, WireError> {
        let mut out = Vec::new();
        self.encode_wire(&mut out)?;
        Ok(out)
    }
}

/// Decode one payload from a shared cursor using its public read methods.
/// [`Self::from_wire_bytes`] additionally rejects trailing bytes.
///
/// ```
/// use safemesh_crdt::{WireCursor, WireDecode, WireEncode, WireError};
/// #[derive(Debug, PartialEq)]
/// struct Reading(u64);
/// impl WireEncode for Reading {
///     fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
///         out.extend_from_slice(&self.0.to_le_bytes());
///         Ok(())
///     }
/// }
/// impl WireDecode for Reading {
///     fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
///         Ok(Self(cursor.read_u64()?))
///     }
/// }
/// let reading = Reading(42);
/// assert_eq!(Reading::from_wire_bytes(&reading.to_wire_bytes()?)?, reading);
/// # Ok::<(), WireError>(())
/// ```
pub trait WireDecode: Sized {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError>;

    /// Decode a nested payload with a collection budget. Other payload types
    /// retain their ordinary decoder unless they implement this method.
    fn decode_wire_with_collection_limits(
        cursor: &mut WireCursor<'_>,
        _limits: CollectionLimits,
    ) -> Result<Self, WireError> {
        Self::decode_wire(cursor)
    }

    /// Decode a payload nested in an EventLog with its record budget.
    fn decode_wire_with_limits(
        cursor: &mut WireCursor<'_>,
        collections: CollectionLimits,
        _max_records: Option<usize>,
    ) -> Result<Self, WireError> {
        Self::decode_wire_with_collection_limits(cursor, collections)
    }

    /// Decode a complete wire value with an explicit collection ceiling.
    fn from_wire_bytes_with_collection_limits(
        bytes: &[u8],
        limits: CollectionLimits,
    ) -> Result<Self, WireError> {
        let mut cursor = WireCursor::new(bytes);
        let value = Self::decode_wire_with_collection_limits(&mut cursor, limits)?;
        if !cursor.is_empty() {
            return Err(WireError::TrailingBytes);
        }
        Ok(value)
    }

    fn from_wire_bytes(bytes: &[u8]) -> Result<Self, WireError> {
        let mut cursor = WireCursor::new(bytes);
        let value = Self::decode_wire(&mut cursor)?;
        if cursor.is_empty() {
            Ok(value)
        } else {
            Err(WireError::TrailingBytes)
        }
    }
}

impl<D: WireDecode> Record<D> {
    /// Decode a record with an explicit budget for nested G-Set or RGA state.
    pub fn from_wire_bytes_with_collection_limits(
        bytes: &[u8],
        limits: CollectionLimits,
    ) -> Result<Self, WireError> {
        let mut cursor = WireCursor::new(bytes);
        let record = Self::decode_wire_with_collection_limits(&mut cursor, limits)?;
        if !cursor.is_empty() {
            return Err(WireError::TrailingBytes);
        }
        Ok(record)
    }
}

/// Borrowed, bounds-checked reader for canonical wire primitives.
///
/// Integers are little-endian; lengths occupy an unsigned 32-bit field.
/// These widths, byte order and documented consumption/error semantics are
/// public compatibility commitments. Storage and bounds-checking internals
/// remain private; custom payload schemas define their own interpretation.
/// Failed byte reads leave the cursor unchanged. Conversion failures in
/// [`Self::read_len`] consume the length field.
pub struct WireCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WireCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        WireCursor { bytes, offset: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    /// Read one byte, or return `UnexpectedEof` without advancing.
    pub fn read_u8(&mut self) -> Result<u8, WireError> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or(WireError::UnexpectedEof)?;
        self.offset += 1;
        Ok(byte)
    }

    /// Read four bytes as a little-endian `u32`, or return `UnexpectedEof` without advancing.
    pub fn read_u32(&mut self) -> Result<u32, WireError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read eight bytes as a little-endian `u64`, or return `UnexpectedEof` without advancing.
    pub fn read_u64(&mut self) -> Result<u64, WireError> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Read a little-endian `u32` length and convert it to `usize`.
    /// Returns `UnexpectedEof` without advancing for a truncated field.
    /// If the value cannot fit `usize`, consumes four bytes and returns `LengthOverflow`.
    pub fn read_len(&mut self) -> Result<usize, WireError> {
        usize::try_from(self.read_u32()?).map_err(|_| WireError::LengthOverflow)
    }

    /// Borrow and consume exactly `len` bytes, including zero bytes.
    /// Returns `LengthOverflow` if offset + len overflows, otherwise
    /// `UnexpectedEof` if the range exceeds the input. Both leave the cursor
    /// unchanged. This operation never allocates or panics for any length.
    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(WireError::LengthOverflow)?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or(WireError::UnexpectedEof)?;
        self.offset = end;
        Ok(bytes)
    }
}

/// Append one byte in the same format as [`WireCursor::read_u8`].
pub fn write_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

/// Append a `u32` as four little-endian bytes, matching [`WireCursor::read_u32`].
pub fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Append a `u64` as eight little-endian bytes, matching [`WireCursor::read_u64`].
pub fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Append `len` as a four-byte little-endian `u32`.
/// Returns `LengthOverflow` without changing `out` if `len` exceeds `u32::MAX`.
/// The result can be read with [`WireCursor::read_len`].
pub fn write_len(out: &mut Vec<u8>, len: usize) -> Result<(), WireError> {
    let len = u32::try_from(len).map_err(|_| WireError::LengthOverflow)?;
    write_u32(out, len);
    Ok(())
}

/// Append a `u32` length prefix followed by the bytes verbatim.
/// Returns `LengthOverflow` without changing `out` if the slice length exceeds `u32::MAX`.
/// Read the prefix with [`WireCursor::read_len`] and the payload with [`WireCursor::read_exact`].
pub fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), WireError> {
    write_len(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

/// Consume one byte and check it against `expected`.
/// A mismatch returns `InvalidTag` after consuming the byte; an empty input
/// returns `UnexpectedEof` without advancing.
pub(super) fn read_tag(cursor: &mut WireCursor<'_>, expected: u8) -> Result<(), WireError> {
    match cursor.read_u8()? {
        tag if tag == expected => Ok(()),
        _ => Err(WireError::InvalidTag),
    }
}

// CRC-32/ISO-HDLC: reflected polynomial, all-ones initialization and final XOR.
// Detects accidental corruption, including every single-byte change; not a MAC.
pub(super) fn frame_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

impl From<LegacyFrame> for WireError {
    fn from(found: LegacyFrame) -> Self {
        Self::LegacyEventLogFrame { found }
    }
}

// After a 0x02 tag, a u32 count of length-prefixed records, each opening with
// the record tag. This is structural only; migration decodes every record. A
// current frame with a damaged tag fails here (its length complement is read
// as a record length) and stays InvalidTag rather than being offered migration.
fn walks_as_tag02_frame(cursor: &WireCursor<'_>) -> bool {
    let mut rest = WireCursor::new(&cursor.bytes[cursor.offset..]);
    let mut walk = || -> Result<(), WireError> {
        for _ in 0..rest.read_len()? {
            let len = rest.read_len()?;
            if rest.read_exact(len)?.first() != Some(&TAG_RECORD) {
                return Err(WireError::InvalidTag);
            }
        }
        Ok(())
    };
    walk().is_ok()
}

// After the 0x03 tag: check the length pair, then the CRC over both length
// fields and the body, and return the body without decoding any of it.
fn read_checked_body<'a>(cursor: &mut WireCursor<'a>) -> Result<&'a [u8], WireError> {
    let start = cursor.offset;
    let len = cursor.read_u32()?;
    if cursor.read_u32()? != !len {
        return Err(WireError::IntegrityMismatch);
    }
    let body = cursor.read_exact(usize::try_from(len).map_err(|_| WireError::LengthOverflow)?)?;
    let checksum = frame_crc32(&cursor.bytes[start..cursor.offset]);
    if cursor.read_u32()? != checksum {
        return Err(WireError::IntegrityMismatch);
    }
    Ok(body)
}

impl<D: WireEncode + WireSchema> EventLog<D> {
    /// Encode an inert batch without admitting it into a live log.
    ///
    /// This preserves input occurrences, including duplicates and invalid records.
    /// Receivers must use `records_from_wire_bytes_for` or `from_wire_bytes_for`
    /// before applying the payload. Encoding does not validate a CRDT domain.
    pub fn encode_records(
        replica_count: Option<usize>,
        records: &[Record<D>],
        out: &mut Vec<u8>,
    ) -> Result<(), WireError> {
        if D::REQUIRES_ARITY && replica_count.is_none() {
            return Err(WireError::MissingShape);
        }
        let mut body = Vec::new();
        write_u32(&mut body, u32::MAX);
        write_bytes(&mut body, D::wire_schema().as_ref())?;
        match replica_count {
            Some(count) => {
                write_u8(&mut body, 1);
                write_u64(
                    &mut body,
                    u64::try_from(count).map_err(|_| WireError::LengthOverflow)?,
                );
            }
            None => write_u8(&mut body, 0),
        }
        write_len(&mut body, records.len())?;
        for record in records {
            write_bytes(&mut body, &record.to_wire_bytes()?)?;
        }
        let len = u32::try_from(body.len()).map_err(|_| WireError::LengthOverflow)?;
        write_u8(out, TAG_EVENT_LOG);
        let start = out.len();
        write_u32(out, len);
        write_u32(out, !len);
        out.extend_from_slice(&body);
        let checksum = frame_crc32(&out[start..]);
        write_u32(out, checksum);
        Ok(())
    }
}

impl<D: WireDecode + WireSchema + PartialEq> EventLog<D> {
    pub(super) fn decode_with<E: From<WireError>>(
        cursor: &mut WireCursor<'_>,
        collection_limits: CollectionLimits,
        nested_max_records: Option<usize>,
        mut occurrence: impl FnMut(&Record<D>),
        mut before_record: impl FnMut(usize) -> Result<(), E>,
    ) -> Result<Self, E> {
        match cursor.read_u8()? {
            TAG_EVENT_LOG => {}
            TAG_EVENT_LOG_LEGACY if walks_as_tag02_frame(cursor) => {
                return Err(WireError::from(LegacyFrame::Tag02).into())
            }
            _ => return Err(WireError::InvalidTag.into()),
        }
        let mut body = WireCursor::new(read_checked_body(cursor)?);
        if body.read_u32()? != u32::MAX {
            return Err(WireError::from(LegacyFrame::Tag03Unshaped).into());
        }
        let schema_len = body.read_len()?;
        if body.read_exact(schema_len)? != D::wire_schema().as_ref() {
            return Err(WireError::DeltaTypeMismatch.into());
        }
        let replica_count = match body.read_u8()? {
            0 if !D::REQUIRES_ARITY => None,
            0 => return Err(WireError::MissingShape.into()),
            1 => Some(usize::try_from(body.read_u64()?).map_err(|_| WireError::LengthOverflow)?),
            _ => return Err(WireError::ArityKindMismatch.into()),
        };
        let mut log = EventLog {
            replica_count,
            shape_bound: true,
            ..EventLog::new()
        };
        for index in 0..body.read_len()? {
            before_record(index)?;
            let record_len = body.read_len()?;
            let record_bytes = body.read_exact(record_len)?;
            let mut record_cursor = WireCursor::new(record_bytes);
            let record = Record::<D>::decode_wire_with_limits(
                &mut record_cursor,
                collection_limits,
                nested_max_records,
            )?;
            if !record_cursor.is_empty() {
                return Err(WireError::TrailingBytes.into());
            }
            occurrence(&record);
            match log.identity_admission(&record) {
                Admission::Collision => return Err(WireError::RecordCollision.into()),
                Admission::Accepted => log.commit_record(record),
                Admission::Duplicate => {}
                Admission::Invalid(_) => unreachable!("identity check does not validate a carrier"),
            }
        }
        if !body.is_empty() {
            return Err(WireError::TrailingBytes.into());
        }
        Ok(log)
    }
}
