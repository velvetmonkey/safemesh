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
