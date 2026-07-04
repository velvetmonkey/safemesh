import { describe, expect, it } from 'vitest'
import { applyGCounterDelta, bottomGCounter, bumpDelta, mergeGCounter, readGCounter } from './gcounter'

describe('G-Counter mirror', () => {
  it('is order-insensitive and redelivery-idempotent', () => {
    const deltas = [bumpDelta(1, 5), bumpDelta(0, 2), bumpDelta(1, 5), bumpDelta(3, 4)]
    const forward = deltas.reduce(applyGCounterDelta, bottomGCounter(4))
    const reverse = [...deltas].reverse().reduce(applyGCounterDelta, bottomGCounter(4))

    expect(forward).toEqual([2, 5, 0, 4])
    expect(reverse).toEqual(forward)
    expect(readGCounter(forward)).toBe(11)
  })

  it('split delivery merge matches receiving all deltas', () => {
    const all = [bumpDelta(0, 3), bumpDelta(2, 9), bumpDelta(0, 6)]
    const full = all.reduce(applyGCounterDelta, bottomGCounter(4))
    const left = [all[0], all[2]].reduce(applyGCounterDelta, bottomGCounter(4))
    const right = [all[1]].reduce(applyGCounterDelta, bottomGCounter(4))

    expect(mergeGCounter(left, right)).toEqual(full)
    expect(full).toEqual([6, 0, 9, 0])
  })
})
