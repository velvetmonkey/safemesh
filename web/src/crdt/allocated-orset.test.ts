import { describe, expect, it } from 'vitest'
import { SafeMeshStringOrSetReplica as Replica } from '../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm'

describe('allocated OR-Set lifecycle', () => {
  it('restarts and adds without a caller token, then removes and merges a peer', () => {
    const left = Replica.createAllocated(2n, 0n)
    const peer = Replica.createAllocated(2n, 1n)
    const first = left.appendAllocatedAdd('water')
    const saved = left.exportIdentity()
    left.free()
    const restored = Replica.importIdentity(saved)
    const next = restored.appendAllocatedAdd('radio')
    const firstView = Replica.inspectRecordBytes(first)
    const nextView = Replica.inspectRecordBytes(next)
    expect(nextView.token()).not.toBe(firstView.token())
    restored.appendRemoveObserved('water')
    restored.mergeRecordBytes(peer.appendAllocatedAdd('water'))
    expect(restored.elements()).toEqual(['radio', 'water'])
    firstView.free()
    nextView.free()
    restored.free()
    peer.free()
  })

  it('refuses a duplicate author and an import with lost allocation state', () => {
    const first = Replica.createAllocated(2n, 0n)
    first.appendAllocatedAdd('water')
    expect(() => Replica.createAllocated(2n, 0n)).toThrow()
    const saved = first.exportIdentity()
    expect(() => Replica.importIdentity(saved)).toThrow(/live allocated writer/)
    first.free()
    const lostSequence = saved.slice()
    new DataView(lostSequence.buffer).setBigUint64(21, 1n, true)
    expect(() => Replica.importIdentity(lostSequence)).toThrow(/next sequence mismatch/)
    // No missing-state fallback may construct a writable fresh replica.
    expect(() => Replica.importIdentity(new Uint8Array())).toThrow()
    const restored = Replica.importIdentity(saved)
    const next = Replica.inspectRecordBytes(restored.appendAllocatedAdd('radio'))
    expect(next.sequence()).toBe(2n)
    next.free()
    restored.free()
  })
})

// Mutate only local identity-storage metadata. No wire fixture is rewritten.
function identityWord(bytes: Uint8Array, offset: number, word: bigint) {
  const copy = bytes.slice()
  new DataView(copy.buffer).setBigUint64(offset, word, true)
  return copy
}

describe('allocated OR-Set boundaries', () => {
  it('validates writer count and author without bigint truncation', () => {
    for (const [writers, author] of [[0n, 0n], [2n, 2n], [2n, 3n], [-1n, 0n],
      [2n, -1n], [1n << 64n, 0n], [2n, 1n << 64n]]) {
      expect(() => Replica.createAllocated(writers, author)).toThrow()
    }
    // Runtime JS inputs still have to be bigint, even if a caller bypasses tsc.
    expect(() => Replica.createAllocated(2 as unknown as bigint, 0n)).toThrow()
  })

  it('refuses malformed, inconsistent and exhausted imports without claiming an author', () => {
    const first = Replica.createAllocated(2n, 0n)
    first.appendAllocatedAdd('水 💧')
    const saved = first.exportIdentity()
    first.free()
    for (const bad of [saved.slice(0, 20), identityWord(saved, 5, 0n),
      identityWord(saved, 5, 3n), identityWord(saved, 13, 2n),
      identityWord(saved, 13, 1n), identityWord(saved, 21, 0n),
      identityWord(saved, 21, (1n << 64n) - 1n), saved.slice(0, -1)]) {
      expect(() => Replica.importIdentity(bad)).toThrow()
    }
    const corrupted = saved.slice()
    corrupted[corrupted.length - 1] ^= 1
    expect(() => Replica.importIdentity(corrupted)).toThrow()
    const restored = Replica.importIdentity(saved)
    expect(restored.elements()).toEqual(['水 💧'])
    restored.appendAllocatedAdd('')
    restored.free()
  })

  it('refuses token overflow before modifying the log', () => {
    const first = Replica.createAllocated((1n << 64n) - 1n, 0n)
    first.appendAllocatedAdd('last token')
    const saved = first.exportIdentity()
    expect(() => first.appendAllocatedAdd('overflow')).toThrow(/exhausted/)
    expect(first.exportIdentity()).toEqual(saved)
    first.free()
    const restored = Replica.importIdentity(saved)
    expect(() => restored.appendAllocatedAdd('overflow after restart')).toThrow(/exhausted/)
    restored.appendRemoveObserved('last token')
    expect(restored.elements()).toEqual([])
    restored.free()
  })

  it('preserves caller tokens and refuses to allocate from incompatible legacy history', () => {
    const legacy = new Replica(0n)
    legacy.appendAdd('water', 77n)
    expect(() => legacy.appendAllocatedAdd('radio')).toThrow(/no allocated identity/)
    expect(() => legacy.exportIdentity()).toThrow(/no allocated identity/)
    const allocated = Replica.createAllocated(2n, 0n)
    allocated.appendAdd('water', 77n)
    expect(allocated.observedTokens('water')).toEqual(new BigUint64Array([77n]))
    expect(() => allocated.appendAllocatedAdd('radio')).toThrow(/token mismatch/)
    expect(() => allocated.exportIdentity()).toThrow(/token mismatch/)
    allocated.free()
    legacy.free()
  })

  it('preflights peer ownership before admission and rejects another writer claiming its author', () => {
    const left = Replica.createAllocated(2n, 0n)
    const peer = Replica.createAllocated(2n, 1n)
    const peerAdd = peer.appendAllocatedAdd('peer')
    expect(left.mergeRecordBytes(peerAdd)).toBe('accepted')
    expect(left.mergeRecordBytes(peerAdd)).toBe('duplicate')
    const impostor = new Replica(0n)
    expect(() => left.mergeRecordBytes(impostor.appendAdd('clone', 2n))).toThrow(/local author/)
    const bad = new Replica(1n)
    bad.appendAdd('valid prefix', 3n)
    bad.appendAdd('wrong token', 99n)
    const before = left.logBytes()
    expect(() => left.mergeLogBytes(bad.logBytes())).toThrow(/token mismatch/)
    expect(left.logBytes()).toEqual(before)
    const snapshot = left.exportIdentity()
    left.free()
    const restored = Replica.importIdentity(snapshot)
    restored.appendRemoveObserved('peer')
    restored.mergeRecordBytes(peer.appendAllocatedAdd('peer'))
    expect(restored.elements()).toEqual(['peer'])
    restored.free()
    peer.free()
    impostor.free()
    bad.free()
  })

  it('documents the scoped guarantee: a self-consistent stale snapshot is accepted', () => {
    const first = Replica.createAllocated(2n, 0n)
    first.appendAllocatedAdd('water')
    const stale = first.exportIdentity()
    first.appendAllocatedAdd('later')
    first.free()
    const restored = Replica.importIdentity(stale)
    expect(restored.elements()).toEqual(['water'])
    restored.free()
  })
})
