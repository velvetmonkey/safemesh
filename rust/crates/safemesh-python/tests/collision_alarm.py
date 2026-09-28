# SafeMesh — delta-state CRDT convergence, built on crdt-lean.
# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
# Executed by the Rust test harness against the actual registered PyO3 module.
# The record-ID collision alarm reaches Python on both replicas of a fork.


def raises(call, text):
    try:
        call()
    except ValueError as error:
        assert str(error) == text, (str(error), text)
        return
    raise AssertionError(("expected ValueError", text))


def exchange(offerer, receiver):
    """Offerer sends its log; receiver merges it and reports back."""
    admissions = receiver.merge_log_bytes(offerer.log_bytes())
    report = receiver.collision_report_bytes()
    verdicts = None if report is None else offerer.merge_collision_report_bytes(report)
    return admissions, verdicts


# The measured fork: author 1 writes tally 5 on one replica and 9 on another.
left, right = sm.GCounterReplica(1, 2), sm.GCounterReplica(1, 2)
five, nine = left.append_bump(1, 5), right.append_bump(1, 9)
assert left.state() == [0, 5] and right.state() == [0, 9]
assert left.version_for(1) == right.version_for(1) == 1
assert left.collisions() == [] and right.collisions() == []
assert left.collision_report_bytes() is None

assert exchange(right, left) == (["collision"], ["recorded"])
for replica, local, remote in [(left, five, nine), (right, nine, five)]:
    (alarm,) = replica.collisions()
    assert isinstance(alarm, sm.RecordCollision)
    assert (alarm.author(), alarm.sequence()) == (1, 1)
    assert (alarm.local(), alarm.remote()) == (local, remote)
    assert repr(alarm) == "RecordCollision(author=1, sequence=1)"
# The alarm never merges either payload; repeating the exchange is idempotent.
assert left.state() == [0, 5] and right.state() == [0, 9]
assert exchange(right, left) == (["collision"], ["known"])
assert exchange(left, right) == (["collision"], ["known"])

# Control: an equal payload is a Duplicate and raises nothing.
left, right = sm.GCounterReplica(1, 2), sm.GCounterReplica(1, 2)
left.append_bump(1, 5)
right.append_bump(1, 5)
assert exchange(right, left) == (["duplicate"], None)
assert exchange(left, right) == (["duplicate"], None)
assert left.collisions() == right.collisions() == []
assert left.state() == right.state() == [0, 5]

# Every replica class raises the alarm on both sides.
forks = [
    (lambda: sm.GCounterReplica(1, 2), lambda r, v: r.append_bump(1, v)),
    (lambda: sm.EnableWinsFlagReplica(1), lambda r, v: r.append_enable(v)),
    (lambda: sm.LwwRegisterReplica(1), lambda r, v: r.append_set(10, 1, v)),
    (lambda: sm.LwwMapReplica(1), lambda r, v: r.append_set(7, 10, 1, v)),
    (lambda: sm.StringOrSetReplica(1), lambda r, v: r.append_add("x", v)),
]
for make, write in forks:
    left, right = make(), make()
    local, remote = write(left, 5), write(right, 9)
    assert right.merge_record_bytes(local) == "collision"
    report = right.collision_report_bytes()
    assert isinstance(report, bytes) and report[0] == 0x05
    assert left.merge_collision_report_bytes(report) == ["recorded"]
    assert [(c.local(), c.remote()) for c in left.collisions()] == [(local, remote)]
    assert [(c.local(), c.remote()) for c in right.collisions()] == [(remote, local)]
    # A peer that never held the ID learns nothing it cannot check.
    stranger = make()
    assert stranger.merge_collision_report_bytes(report) == ["unheld"]
    assert stranger.collisions() == []

# A report is refused whole, with its cause named, and changes nothing.
left, right = sm.GCounterReplica(1, 2), sm.GCounterReplica(1, 2)
left.append_bump(1, 5)
right.append_bump(1, 9)
assert right.merge_log_bytes(left.log_bytes()) == ["collision"]
report = right.collision_report_bytes()
raises(lambda: left.merge_collision_report_bytes(b""),
       "failed to decode collision report: unexpected end of wire input")
raises(lambda: left.merge_collision_report_bytes(report[:-1]),
       "failed to decode collision report: unexpected end of wire input")
raises(lambda: left.merge_collision_report_bytes(report[:-1] + bytes([report[-1] ^ 1])),
       "failed to decode collision report: wire frame integrity check failed")
raises(lambda: left.merge_collision_report_bytes(report + b"\0"),
       "failed to decode collision report: unexpected trailing bytes after wire value")
raises(lambda: left.merge_collision_report_bytes(left.log_bytes()),
       "failed to decode collision report: unexpected wire tag")
raises(lambda: sm.LwwRegisterReplica(1).merge_collision_report_bytes(report),
       "failed to decode collision report: wire delta schema does not match the expected type")
raises(lambda: left.merge_collision_report_bytes(report, max_records=0),
       "failed to decode collision report: RecordLimitExceeded: 0")
assert left.collisions() == [] and left.state() == [0, 5]
assert left.merge_collision_report_bytes(report, max_records=1) == ["recorded"]

# What a peer without the report surface sees: its record and log decoders
# refuse the new tag before reading further, and nothing changes.
peer = sm.GCounterReplica(1, 2)
peer.append_bump(1, 5)
before = peer.log_bytes()
raises(lambda: peer.merge_record_bytes(report),
       "failed to decode record: unexpected wire tag")
raises(lambda: peer.merge_log_bytes(report),
       "failed to decode event log: unexpected wire tag")
assert peer.log_bytes() == before and peer.state() == [0, 5]
