# SafeMesh — delta-state CRDT convergence, built on crdt-lean.
# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
# Executed by the Rust test harness against the actual registered PyO3 module.
# Mirrors the WASM SafeMeshStringOrSetReplica host tests through the Python surface.


def raises(kind, call, message=None):
    try:
        call()
    except kind as error:
        if message is not None:
            assert str(error) == message, (str(error), message)
        return
    raise AssertionError(('expected exception', kind))


# C1: records round-trip through the core; a third replica repairs from the log.
left, right = sm.StringOrSetReplica(1), sm.StringOrSetReplica(2)
add = left.append_add('vaccine', 11)
assert isinstance(add, bytes)
assert right.merge_record_bytes(add) == 'accepted'
assert right.elements() == left.elements() == ['vaccine']
assert right.observed_tokens('vaccine') == [11]
assert right.merge_record_bytes(left.append_remove_observed('vaccine')) == 'accepted'
assert right.elements() == left.elements() == []
assert right.tombstones() == left.tombstones() == [11]
assert right.add_entries() == left.add_entries() == [('vaccine', 11)]
third = sm.StringOrSetReplica(3)
assert third.merge_log_bytes(left.log_bytes()) == ['accepted', 'accepted']
assert third.merge_log_bytes(left.log_bytes()) == ['duplicate', 'duplicate']
assert (third.elements(), third.tombstones(), third.add_entries()) == (
    left.elements(), left.tombstones(), left.add_entries())
for replica in [left, right, third]:
    assert (replica.version_for(1), replica.version_for(2)) == (2, 0)

# C2: a duplicate does not move state; same identity, other payload is a
# collision verdict on both merge paths, and neither reading is absorbed.
author, reader = sm.StringOrSetReplica(1), sm.StringOrSetReplica(2)
record = author.append_add('vaccine', 11)
assert reader.merge_record_bytes(record) == 'accepted'
assert reader.merge_record_bytes(record) == 'duplicate'
assert (reader.elements(), reader.version_for(1)) == (['vaccine'], 1)
forger = sm.StringOrSetReplica(1)
forged = forger.append_add('forged', 99)
assert reader.merge_record_bytes(forged) == 'collision'
assert reader.merge_log_bytes(forger.log_bytes()) == ['collision']
assert (reader.elements(), reader.add_entries()) == (['vaccine'], [('vaccine', 11)])

# C3: corrupted bytes raise the WASM error text and leave the reader untouched.
planted = bytes([record[0] ^ 0xff]) + record[1:]
reader = sm.StringOrSetReplica(2)
raises(ValueError, lambda: reader.merge_record_bytes(planted),
       'failed to decode record: unexpected wire tag')
raises(ValueError, lambda: sm.StringOrSetReplica.inspect_record_bytes(planted),
       'failed to decode record: unexpected wire tag')
log = author.log_bytes()
for position in range(len(log)):
    bad = bytearray(log)
    bad[position] ^= 0x01
    raises(ValueError, lambda: reader.merge_log_bytes(bytes(bad)))
    assert reader.elements() == [] and reader.version_for(1) == 0

# C4: core token semantics, including a reused token across elements.
replica = sm.StringOrSetReplica(1)
replica.append_add('a', 7)
replica.append_add('b', 7)
assert replica.elements() == ['a', 'b']
assert replica.add_entries() == [('a', 7), ('b', 7)]
replica.append_remove_observed('a')
assert replica.elements() == [] and replica.tombstones() == [7]
assert replica.observed_tokens('a') == []

# Inspector: fields decoded by the core, nothing admitted anywhere.
author = sm.StringOrSetReplica(9)
add = author.append_add('vaccine', 11)
remove = author.append_remove_observed('vaccine')
view = sm.StringOrSetReplica.inspect_record_bytes(add)
assert (view.replica(), view.sequence(), view.delta_kind(), view.element(),
        view.token(), view.tokens()) == (9, 1, 'add', 'vaccine', 11, [])
view = sm.StringOrSetReplica.inspect_record_bytes(remove)
assert (view.replica(), view.sequence(), view.delta_kind(), view.element(),
        view.token(), view.tokens()) == (9, 2, 'remove', None, None, [11])
assert sm.StringOrSetReplica(2).merge_record_bytes(add) == 'accepted'

# UTF-8 elements and full u64 tokens cross the boundary unchanged.
maximum = (1 << 64) - 1
left, right = sm.StringOrSetReplica(maximum), sm.StringOrSetReplica(0)
assert right.merge_record_bytes(left.append_add('café ☃', maximum)) == 'accepted'
assert right.elements() == ['café ☃']
assert right.add_entries() == [('café ☃', maximum)]
assert right.version_for(maximum) == 1

# The collection budget is keyword-only and uses the WASM error text.
wide = sm.StringOrSetReplica(1)
for token in [1, 2, 3]:
    wide.append_add('x', token)
remove = wide.append_remove_observed('x')
reader = sm.StringOrSetReplica(2)
for call in [lambda: reader.merge_record_bytes(remove, max_collection_elements=2),
             lambda: sm.StringOrSetReplica.inspect_record_bytes(remove, max_collection_elements=2),
             lambda: reader.merge_log_bytes(wide.log_bytes(), max_collection_elements=2)]:
    raises(ValueError, call, 'maxCollectionElements limit exceeded: 2')
assert reader.log_bytes() == sm.StringOrSetReplica(2).log_bytes()
raises(TypeError, lambda: reader.merge_record_bytes(remove, 2))
assert reader.merge_record_bytes(remove, max_collection_elements=3) == 'accepted'
assert reader.merge_log_bytes(wide.log_bytes(), max_collection_elements=None) == [
    'accepted', 'accepted', 'accepted', 'duplicate']

# Numeric arguments reject bools and out-of-range integers without mutation.
replica = sm.StringOrSetReplica(1)
for bad in [True, False, 0.5, -1, 1 << 64]:
    raises((TypeError, OverflowError), lambda: sm.StringOrSetReplica(bad))
    raises((TypeError, OverflowError), lambda: replica.append_add('x', bad))
    raises((TypeError, OverflowError), lambda: replica.version_for(bad))
    raises((TypeError, OverflowError),
           lambda: replica.merge_record_bytes(add, max_collection_elements=bad))
raises(TypeError, lambda: replica.append_add(b'x', 1))
assert replica.elements() == [] and replica.version_for(1) == 0

# Allocated writers. The registry is process-wide and the Rust test harness runs
# this script beside other tests, so these authors (5000 and up) are unique.
W = 5010
writer = sm.StringOrSetReplica.create_allocated(W, 5000)
first = writer.append_allocated_add('water')
view = sm.StringOrSetReplica.inspect_record_bytes(first)
assert (view.replica(), view.sequence(), view.token()) == (5000, 1, W + 5000)
raises(ValueError, lambda: sm.StringOrSetReplica.create_allocated(W, 5000),
       'author already has a live allocated writer')
raises(ValueError, lambda: writer.append_add('x', 1),
       'allocated replica rejects caller-supplied tokens; use appendAllocatedAdd')
raises(ValueError, lambda: sm.StringOrSetReplica.create_allocated(5001, 5001),
       'invalid writer configuration')
plain = sm.StringOrSetReplica(5002)
raises(ValueError, lambda: plain.append_allocated_add('x'), 'replica has no allocated identity')
raises(ValueError, lambda: plain.export_identity(), 'replica has no allocated identity')
raises(ValueError, lambda: sm.StringOrSetReplica.import_identity(b'SMOI'),
       'allocation/history consistency: invalid identity storage')
for bad in [True, -1, 1 << 64]:
    raises((TypeError, OverflowError), lambda: sm.StringOrSetReplica.create_allocated(bad, 1))
    raises((TypeError, OverflowError), lambda: sm.StringOrSetReplica.create_allocated(W, bad))
saved = writer.export_identity()
assert isinstance(saved, bytes) and saved[:5] == b'SMOI\x01'
raises(ValueError, lambda: sm.StringOrSetReplica.import_identity(saved),
       'author already has a live allocated writer')
del writer, view
restored = sm.StringOrSetReplica.import_identity(saved)
second = restored.append_allocated_add('radio')
assert sm.StringOrSetReplica.inspect_record_bytes(second).token() == 2 * W + 5000
assert restored.elements() == ['radio', 'water']
peer = sm.StringOrSetReplica.create_allocated(W, 5003)
assert peer.merge_log_bytes(restored.log_bytes()) == ['accepted', 'accepted']
raises(ValueError, lambda: peer.merge_record_bytes(add),
       'allocation/history consistency: token mismatch')
assert peer.elements() == ['radio', 'water']
del restored, peer

# Public lifecycle: claims depend only on author, and last-reference drop on
# another thread releases the process-wide claim for both creation and import.
import threading
import queue

owner = sm.StringOrSetReplica.create_allocated(7010, 7000)
identity = owner.export_identity()
for writers in [7010, 7011]:
    raises(ValueError, lambda: sm.StringOrSetReplica.create_allocated(writers, 7000),
           'author already has a live allocated writer')
raises(ValueError, lambda: sm.StringOrSetReplica.import_identity(identity),
       'author already has a live allocated writer')
other = sm.StringOrSetReplica.create_allocated(7010, 7001)
assert other.append_allocated_add('peer')

handoff = queue.Queue()
handoff.put(owner)
del owner

def drop_on_worker():
    last_reference = handoff.get()
    del last_reference

worker = threading.Thread(target=drop_on_worker)
worker.start()
worker.join()
assert handoff.empty()
replacement = sm.StringOrSetReplica.create_allocated(7010, 7000)
raises(ValueError, lambda: sm.StringOrSetReplica.import_identity(identity),
       'author already has a live allocated writer')
del replacement
restored = sm.StringOrSetReplica.import_identity(identity)
raises(ValueError, lambda: sm.StringOrSetReplica.create_allocated(7010, 7000),
       'author already has a live allocated writer')
assert restored.append_allocated_add('restored')
del restored, other

# REPLICA_DIFFERENTIAL
# Frozen from untouched product main 6cfcee7; never regenerate from the migration.
import json
import struct
import zlib
import threading

def replica_differential():
    rows = []
    def snapshot(r):
        state = ([r.state(), r.value()] if isinstance(r, sm.GCounterReplica)
                 else [r.elements(), r.add_entries(), r.tombstones(),
                       [r.observed_tokens(x) for x in r.elements()]])
        return [state, r.log_bytes().hex(),
                [r.version_for(x) for x in [0, 1, 2, 6010, 6011, 6012]]]
    def call(label, r, f):
        try:
            value = f()
            result = ['ok', value.hex() if isinstance(value, bytes) else value]
        except Exception as error:
            result = ['error', type(error).__name__, str(error)]
        rows.append([label, result, snapshot(r)])
    def frame(template, records):
        # Retain the product's schema and shape, replace the occurrence section,
        # and compute the frame's specified CRC rather than copying a fixture.
        body = template[9:-4]
        n = struct.unpack_from('<I', body, 4)[0]
        shape = 8 + n
        end = shape + (9 if body[shape] else 1)
        body = body[:end] + struct.pack('<I', len(records))
        for record in records:
            body += struct.pack('<I', len(record)) + record
        header = struct.pack('<II', len(body), len(body) ^ 0xffffffff)
        return template[:1] + header + body + struct.pack('<I', zlib.crc32(header + body))
    def identity(record, author=None, sequence=None):
        record = bytearray(record)
        if author is not None:
            struct.pack_into('<Q', record, 1, author)
        if sequence is not None:
            struct.pack_into('<Q', record, 9, sequence)
        return bytes(record)
    g0, g1 = sm.GCounterReplica(0, 3), sm.GCounterReplica(1, 3)
    a, b = g0.append_bump(0, 5), g1.append_bump(1, 7)
    gf = sm.GCounterReplica(0, 3)
    collision = gf.append_bump(0, 99)
    g = sm.GCounterReplica(2, 3)
    good = frame(g0.log_bytes(), [a, b])
    for label, f in [
        ('g.valid', lambda: g.merge_log_bytes(good)),
        ('g.duplicate', lambda: g.merge_log_bytes(frame(good, [a, a, b]))),
        ('g.collision.record', lambda: g.merge_record_bytes(collision)),
        ('g.collision.log', lambda: g.merge_log_bytes(gf.log_bytes())),
        ('g.internal-collision', lambda: g.merge_log_bytes(frame(good, [a, collision]))),
        ('g.not-owned.record', lambda: g.merge_record_bytes(identity(a, author=1))),
        ('g.invalid', lambda: g.merge_record_bytes(bytes([255]))),
        ('g.not-owned.batch', lambda: g.merge_log_bytes(frame(good, [identity(b, sequence=2), identity(a, author=1)]))),
        ('g.over-budget', lambda: g.merge_log_bytes(frame(good, [a, a, b]), max_records=2)),
        ('g.truncated', lambda: g.merge_log_bytes(good[:-1])),
        ('g.trailing', lambda: g.merge_log_bytes(good + b'x')),
        ('g.record-trailing', lambda: g.merge_record_bytes(a + b'x')),
        ('g.bool', lambda: g.append_bump(True, 9)),
    ]:
        call(label, g, f)
    # Two distinct counter coordinates make loss of the first replay observable.
    log = g.log_bytes()
    del g
    g = sm.GCounterReplica(2, 3)
    call('g.restore', g, lambda: g.merge_log_bytes(log))
    call('g.exchange-again', g, lambda: g.merge_record_bytes(g1.append_bump(1, 11)))
    g1.merge_log_bytes(g.log_bytes())
    call('g.final', g, lambda: [g1.state(), g1.log_bytes().hex()])

    s0, s1 = sm.StringOrSetReplica(0), sm.StringOrSetReplica(1)
    sa, sb = s0.append_add('café', 11), s1.append_add('water', 12)
    sf = sm.StringOrSetReplica(0)
    sc = sf.append_add('forged', 99)
    s = sm.StringOrSetReplica(2)
    sg = frame(s0.log_bytes(), [sa, sb])
    wide = sm.StringOrSetReplica(1)
    wide.append_add('wide', 30)
    wide.append_add('wide', 31)
    remove = wide.append_remove_observed('wide')
    zero = identity(remove, sequence=0)
    for label, f in [
        ('s.valid', lambda: s.merge_log_bytes(sg)),
        ('s.duplicate', lambda: s.merge_log_bytes(frame(sg, [sa, sa, sb]))),
        ('s.collision.record', lambda: s.merge_record_bytes(sc)),
        ('s.collision.log', lambda: s.merge_log_bytes(sf.log_bytes())),
        ('s.internal-collision', lambda: s.merge_log_bytes(frame(sg, [sa, sc]))),
        ('s.invalid.record', lambda: s.merge_record_bytes(zero)),
        ('s.invalid.batch', lambda: s.merge_log_bytes(frame(sg, [sb, zero]))),
        ('s.over-budget.record', lambda: s.merge_record_bytes(remove, max_collection_elements=1)),
        ('s.over-budget.log', lambda: s.merge_log_bytes(wide.log_bytes(), max_collection_elements=1)),
        ('s.truncated', lambda: s.merge_log_bytes(sg[:-1])),
        ('s.trailing', lambda: s.merge_log_bytes(sg + b'x')),
        ('s.record-trailing', lambda: s.merge_record_bytes(sa + b'x')),
        ('s.wrong-schema', lambda: s.merge_log_bytes(good)),
        ('s.inspect', lambda: [sm.StringOrSetReplica.inspect_record_bytes(sa).element(),
                               sm.StringOrSetReplica.inspect_record_bytes(sa).token()]),
    ]:
        call(label, s, f)
    call('g.wrong-schema', g, lambda: g.merge_log_bytes(sg))
    log = s.log_bytes()
    del s
    s = sm.StringOrSetReplica(2)
    call('s.restore', s, lambda: s.merge_log_bytes(log))
    call('s.exchange-again', s, lambda: s.merge_record_bytes(s1.append_remove_observed('water')))
    s1.merge_log_bytes(s.log_bytes())
    call('s.final', s, lambda: [s1.elements(), s1.log_bytes().hex()])

    W = 6020
    left = sm.StringOrSetReplica.create_allocated(W, 6010)
    peer = sm.StringOrSetReplica.create_allocated(W, 6012)
    ra = left.append_allocated_add('first writer')
    rb = peer.append_allocated_add('second writer')
    left.merge_record_bytes(rb)
    peer.merge_record_bytes(ra)
    saved = left.export_identity()
    call('allocated.identity', left, lambda: saved)
    call('allocated.live-import', left, lambda: sm.StringOrSetReplica.import_identity(saved))
    # Drop the last reference on another thread, then re-claim on this thread.
    holder = [left]
    del left
    thread = threading.Thread(target=lambda: holder.pop())
    thread.start()
    thread.join()
    left = sm.StringOrSetReplica.import_identity(saved)
    call('allocated.restore-two-writers', left, lambda: left.export_identity())
    rc = left.append_allocated_add('after restart')
    call('allocated.exchange-again', peer, lambda: peer.merge_record_bytes(rc))
    left.merge_log_bytes(peer.log_bytes())
    call('allocated.final', left, lambda: [peer.elements(), peer.log_bytes().hex()])
    receiver = sm.StringOrSetReplica.create_allocated(W, 6011)
    # Fix the add token to the new author so refusal is the local-author rule.
    forger = sm.StringOrSetReplica(6011)
    unknown_local = forger.append_add('local forged', W + 6011)
    call('allocated.not-owned', receiver, lambda: receiver.merge_record_bytes(sa))
    call('allocated.refused-batch', receiver, lambda: receiver.merge_log_bytes(frame(sg, [rb, unknown_local])))
    call('allocated.bad-next', receiver, lambda: sm.StringOrSetReplica.import_identity(
        saved[:21] + struct.pack('<Q', 99) + saved[29:]))
    del left, peer, receiver
    # JSON normalization makes tuples/lists identical to the frozen JSON table.
    return json.loads(json.dumps(rows, ensure_ascii=False))

actual = replica_differential()
# Expected table captured once from unchanged main product code.
expected = [['g.valid',
  ['ok', ['accepted', 'accepted']],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.duplicate',
  ['ok', ['duplicate', 'duplicate', 'duplicate']],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.collision.record',
  ['ok', 'collision'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.collision.log',
  ['ok', ['collision']],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.internal-collision',
  ['error', 'ValueError', 'record ID collision'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.not-owned.record',
  ['error', 'ValueError', 'counter coordinate out of range or not owned by record author'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.invalid',
  ['error', 'ValueError', 'failed to decode record: unexpected wire tag'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.not-owned.batch',
  ['error', 'ValueError', 'counter coordinate out of range or not owned by record author'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.over-budget',
  ['error', 'ValueError', 'failed to decode event log: RecordLimitExceeded: 2'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.truncated',
  ['error', 'ValueError', 'failed to decode event log: unexpected end of wire input'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.trailing',
  ['error', 'ValueError', 'failed to decode event log: unexpected trailing bytes after wire value'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.record-trailing',
  ['error', 'ValueError', 'failed to decode record: unexpected trailing bytes after wire value'],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.bool',
  ['error', 'TypeError', "argument 'counter_replica': expected an integer, got bool"],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.restore',
  ['ok', ['accepted', 'accepted']],
  [[[5, 7, 0], 12],
   '03830000007cffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000002000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000f3706655',
   [1, 1, 0, 0, 0, 0]]],
 ['g.exchange-again',
  ['ok', 'accepted'],
  [[[5, 11, 0], 16],
   '03ad00000052ffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000003000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000260000000101000000000000000200000000000000110000001001000000000000000b00000000000000ad28b93f',
   [1, 2, 0, 0, 0, 0]]],
 ['g.final',
  ['ok',
   [[5, 11, 0],
    '03ad00000052ffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000003000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000260000000101000000000000000200000000000000110000001001000000000000000b000000000000002600000001000000000000000001000000000000001100000010000000000000000005000000000000003a79382a']],
  [[[5, 11, 0], 16],
   '03ad00000052ffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000003000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000260000000101000000000000000200000000000000110000001001000000000000000b00000000000000ad28b93f',
   [1, 2, 0, 0, 0, 0]]],
 ['s.valid',
  ['ok', ['accepted', 'accepted']],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.duplicate',
  ['ok', ['duplicate', 'duplicate', 'duplicate']],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.collision.record',
  ['ok', 'collision'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.collision.log',
  ['ok', ['collision']],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.internal-collision',
  ['error', 'ValueError', 'record ID collision'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.invalid.record',
  ['error', 'ValueError', 'invalid record'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.invalid.batch',
  ['error',
   'ValueError',
   'failed to decode event log: OR-Set remove record (replica 1, sequence 0) refused: remove '
   'sequences start at 1. Recovery: remove this record from any stored log, issue the remove again '
   'from replica 1 at a positive sequence, and write the log again'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.over-budget.record',
  ['error', 'ValueError', 'maxCollectionElements limit exceeded: 1'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.over-budget.log',
  ['error', 'ValueError', 'maxCollectionElements limit exceeded: 1'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.truncated',
  ['error', 'ValueError', 'failed to decode event log: unexpected end of wire input'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.trailing',
  ['error', 'ValueError', 'failed to decode event log: unexpected trailing bytes after wire value'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.record-trailing',
  ['error', 'ValueError', 'failed to decode record: unexpected trailing bytes after wire value'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.wrong-schema',
  ['error', 'ValueError', 'delta type mismatch'],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.inspect',
  ['ok', ['café', 11]],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['g.wrong-schema',
  ['error', 'ValueError', 'delta type mismatch'],
  [[[5, 11, 0], 16],
   '03ad00000052ffffffffffffff1a000000736166656d6573682f67636f756e7465722d64656c74612f763101030000000000000003000000260000000100000000000000000100000000000000110000001000000000000000000500000000000000260000000101000000000000000100000000000000110000001001000000000000000700000000000000260000000101000000000000000200000000000000110000001001000000000000000b00000000000000ad28b93f',
   [1, 2, 0, 0, 0, 0]]],
 ['s.restore',
  ['ok', ['accepted', 'accepted']],
  [[['café', 'water'], [['café', 11], ['water', 12]], [], [[11], [12]]],
   '03830000007cffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310002000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000001885bb14',
   [1, 1, 0, 0, 0, 0]]],
 ['s.exchange-again',
  ['ok', 'accepted'],
  [[['café'], [['café', 11], ['water', 12]], [12], [[11]]],
   '03a900000056ffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310003000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000002200000001010000000000000002000000000000000d00000034010000000c000000000000003aa665c0',
   [1, 2, 0, 0, 0, 0]]],
 ['s.final',
  ['ok',
   [['café'],
    '03a900000056ffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f7631000300000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000002200000001010000000000000002000000000000000d00000034010000000c00000000000000270000000100000000000000000100000000000000120000003305000000636166c3a90b00000000000000a5a5a2b5']],
  [[['café'], [['café', 11], ['water', 12]], [12], [[11]]],
   '03a900000056ffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f76310003000000270000000100000000000000000100000000000000120000003305000000636166c3a90b0000000000000027000000010100000000000000010000000000000012000000330500000077617465720c000000000000002200000001010000000000000002000000000000000d00000034010000000c000000000000003aa665c0',
   [1, 2, 0, 0, 0, 0]]],
 ['allocated.identity',
  ['ok',
   '534d4f490184170000000000007a17000000000000020000000000000003920000006dffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100020000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f00000000000008decc9b'],
  [[['first writer', 'second writer'],
    [['first writer', 12030], ['second writer', 12032]],
    [],
    [[12030], [12032]]],
   '03920000006dffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100020000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f00000000000008decc9b',
   [0, 0, 0, 1, 0, 1]]],
 ['allocated.live-import',
  ['error', 'ValueError', 'author already has a live allocated writer'],
  [[['first writer', 'second writer'],
    [['first writer', 12030], ['second writer', 12032]],
    [],
    [[12030], [12032]]],
   '03920000006dffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100020000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f00000000000008decc9b',
   [0, 0, 0, 1, 0, 1]]],
 ['allocated.restore-two-writers',
  ['ok',
   '534d4f490184170000000000007a17000000000000020000000000000003920000006dffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100020000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f00000000000008decc9b'],
  [[['first writer', 'second writer'],
    [['first writer', 12030], ['second writer', 12032]],
    [],
    [[12030], [12032]]],
   '03920000006dffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100020000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f00000000000008decc9b',
   [0, 0, 0, 1, 0, 1]]],
 ['allocated.exchange-again',
  ['ok', 'accepted'],
  [[['after restart', 'first writer', 'second writer'],
    [['after restart', 18050], ['first writer', 12030], ['second writer', 12032]],
    [],
    [[18050], [12030], [12032]]],
   '03c50000003affffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100030000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f0000000000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017a1700000000000002000000000000001a000000330d00000061667465722072657374617274824600000000000084611086',
   [0, 0, 0, 2, 0, 1]]],
 ['allocated.final',
  ['ok',
   [['after restart', 'first writer', 'second writer'],
    '03c50000003affffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100030000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f0000000000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017a1700000000000002000000000000001a000000330d00000061667465722072657374617274824600000000000084611086']],
  [[['after restart', 'first writer', 'second writer'],
    [['after restart', 18050], ['first writer', 12030], ['second writer', 12032]],
    [],
    [[18050], [12030], [12032]]],
   '03c50000003affffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100030000002e000000017a17000000000000010000000000000019000000330c000000666972737420777269746572fe2e0000000000002f000000017c1700000000000001000000000000001a000000330d0000007365636f6e6420777269746572002f0000000000002f000000017a1700000000000002000000000000001a000000330d0000006166746572207265737461727482460000000000005f953df7',
   [0, 0, 0, 2, 0, 1]]],
 ['allocated.not-owned',
  ['error', 'ValueError', 'allocation/history consistency: token mismatch'],
  [[[], [], [], []],
   '032d000000d2ffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100000000001801b0e4',
   [0, 0, 0, 0, 0, 0]]],
 ['allocated.refused-batch',
  ['error', 'ValueError', 'incoming record claims the local author'],
  [[[], [], [], []],
   '032d000000d2ffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100000000001801b0e4',
   [0, 0, 0, 0, 0, 0]]],
 ['allocated.bad-next',
  ['error', 'ValueError', 'allocation/history consistency: next sequence mismatch'],
  [[[], [], [], []],
   '032d000000d2ffffffffffffff20000000736166656d6573682f6f727365742d64656c74612d757466382d7536342f763100000000001801b0e4',
   [0, 0, 0, 0, 0, 0]]]]
assert len(actual) == len(expected)
for got, want in zip(actual, expected):
    assert got == want, ('differential mismatch', got[0], got, want)
