# SafeMesh — delta-state CRDT convergence, built on crdt-lean.
# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
# Executed by the Rust test harness against the actual registered PyO3 module.
import sys


def raises(kind, call):
    try:
        call()
    except kind:
        return
    raise AssertionError(('expected exception', kind))


for reverse in [False, True]:
    left, right = sm.GSet(), sm.GSet()
    assert left.elements() == []
    left.insert(7)
    left.insert(7)
    right.insert(2)
    if reverse:
        right.merge(left)
        left.merge(right)
    else:
        left.merge(right)
        right.merge(left)
    assert left.elements() == right.elements() == [2, 7]
    assert left.contains(7) and not left.contains(9)
    left.merge(left)
    left.insert((1 << 64) - 1)
    assert left.elements() == [2, 7, (1 << 64) - 1]

    left, right = sm.PnCounter(2), sm.PnCounter(2)
    assert left.value() == 0
    left.apply_inc(0, 9)
    left.try_apply_inc(0, 3)  # Absolute monotone tallies, not additive increments.
    right.apply_dec(1, 12)
    right.try_apply_dec(1, 4)
    if reverse:
        right.merge(left)
        left.merge(right)
    else:
        left.merge(right)
        right.merge(left)
    assert left.value() == right.value() == -3
    assert left.p_state() == right.p_state() == [9, 0]
    assert left.n_state() == right.n_state() == [0, 12]
    left.merge(left)
    for method in ['apply_inc', 'try_apply_inc', 'apply_dec', 'try_apply_dec']:
        raises(IndexError, lambda: getattr(left, method)(2, 99))
        assert (left.p_state(), left.n_state()) == ([9, 0], [0, 12])
    for size in [1, 3]:
        raises(ValueError, lambda: left.merge(sm.PnCounter(size)))
        assert (left.p_state(), left.n_state()) == ([9, 0], [0, 12])

    left, right = sm.Rga(), sm.Rga()
    assert left.live_entries() == left.read_positions() == []
    left.insert(2, 20)
    left.insert(1, 11)
    left.insert(1, 10)  # Shared positions retain distinct values in sorted order.
    right.delete(2)
    right.delete(3)  # Deletion may arrive before insertion.
    left.insert(3, 30)
    right.insert(4, 40)
    if reverse:
        right.merge(left)
        left.merge(right)
    else:
        left.merge(right)
        right.merge(left)
    assert left.live_entries() == right.live_entries() == [(1, 10), (1, 11), (4, 40)]
    assert left.read_positions() == right.read_positions() == [1, 1, 4]
    assert left.placed() == right.placed() == [(1, 10), (1, 11), (2, 20), (3, 30), (4, 40)]
    assert left.tombstones() == right.tombstones() == [2, 3]
    left.merge(left)
    left.insert(2, 99)
    assert left.live_entries() == [(1, 10), (1, 11), (4, 40)]

counter = sm.PnCounter(2)
maximum = (1 << 64) - 1
counter.apply_dec(0, maximum)
counter.apply_dec(1, maximum)
assert counter.value() == -2 * maximum
counter.apply_inc(0, maximum)
counter.apply_inc(1, maximum)
assert counter.value() == 0
positive = sm.PnCounter(2)
positive.apply_inc(0, maximum)
positive.apply_inc(1, maximum)
assert positive.value() == 2 * maximum
assert sm.PnCounter(0).value() == 0
raises(ValueError, lambda: sm.PnCounter(sys.maxsize // 8 + 1))

# Every numeric argument uses the crate's strict integer extraction, atomically.
gset, rga, counter = sm.GSet(), sm.Rga(), sm.PnCounter(2)
for bad in [True, False, 0.5, float('nan'), float('inf'), -1, 1 << 64]:
    error = (TypeError, OverflowError)
    raises(error, lambda: sm.PnCounter(bad))
    for call in [lambda: gset.insert(bad), lambda: gset.contains(bad),
                 lambda: rga.insert(bad, 1), lambda: rga.insert(1, bad),
                 lambda: rga.delete(bad)]:
        raises(error, call)
    for method in ['apply_inc', 'try_apply_inc', 'apply_dec', 'try_apply_dec']:
        raises(error, lambda: getattr(counter, method)(bad, 1))
        raises(error, lambda: getattr(counter, method)(0, bad))
    assert gset.elements() == []
    assert rga.placed() == rga.tombstones() == []
    assert counter.p_state() == counter.n_state() == [0, 0]

# Core decoding rejects incomplete payloads, impossible claimed lengths, and
# another CRDT's tag before constructing a Python value.
gset_bytes = sm.GSet().to_wire_bytes()
rga_bytes = sm.Rga().to_wire_bytes()
for cls, payload in [(sm.GSet, gset_bytes), (sm.Rga, rga_bytes)]:
    assert cls.from_wire_bytes(payload).to_wire_bytes() == payload
    raises(ValueError, lambda: cls.from_wire_bytes(payload[:-1]))
    raises(ValueError, lambda: cls.from_wire_bytes(payload[:1] + b'\xff\xff\xff\xff'))
raises(ValueError, lambda: sm.GSet.from_wire_bytes(rga_bytes))
assert not hasattr(sm.PnCounter, 'to_wire_bytes')
assert not hasattr(sm.PnCounter, 'from_wire_bytes')


def retention_python_merge_log_bytes_all_5000():
    import struct

    def identities(frame):
        offset = 17 + struct.unpack_from('<I', frame, 13)[0]
        shape = frame[offset]
        offset += 1 + (8 if shape else 0)
        count = struct.unpack_from('<I', frame, offset)[0]
        offset += 4
        result = []
        for _ in range(count):
            length = struct.unpack_from('<I', frame, offset)[0]
            offset += 4
            result.append(struct.unpack_from('<QQ', frame, offset + 1))
            offset += length
        return sorted(result)

    authors = [sm.LwwMapReplica(author) for author in range(3)]
    source = sm.LwwMapReplica(0)
    expected_ids = []
    for index in range(5000):
        author = index % 3
        record = authors[author].append_set(index, index + 1, author, index + 1)
        assert source.merge_record_bytes(record) == 'accepted'
        expected_ids.append((author, index // 3 + 1))
    log = source.log_bytes()
    target = sm.LwwMapReplica(0)
    target.append_set(999999, 1, 0, 99)
    before = target.log_bytes()
    keys_before = target.visible_keys()
    try:
        target.merge_log_bytes(log, max_records=4999)
    except ValueError as error:
        assert 'RecordLimitExceeded' in str(error), str(error)
    else:
        raise AssertionError('retention budget must refuse, never truncate')
    assert target.log_bytes() == before
    assert target.visible_keys() == keys_before
    target = sm.LwwMapReplica(0)
    assert target.merge_log_bytes(log, max_records=5000) == ['accepted'] * 5000
    assert len(identities(target.log_bytes())) == 5000
    assert identities(target.log_bytes()) == sorted(expected_ids)
    assert target.log_bytes() == log
    assert target.visible_keys() == list(range(5000))
    for index in range(5000):
        assert target.value_or(index, 0) == index + 1
    print('retention_python_merge_log_bytes_all_5000 PASS')


retention_python_merge_log_bytes_all_5000()
