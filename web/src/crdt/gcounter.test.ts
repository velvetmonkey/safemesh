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

it('rejects unequal replica counts in both merge orders', () => {
  expect(() => mergeGCounter([1], [1, 8])).toThrow(RangeError)
  expect(() => mergeGCounter([1, 8], [1])).toThrow(RangeError)
})

it.each([0.5, NaN, Infinity, -1])('rejects invalid replica count %s', (bad) => {
  expect(() => bottomGCounter(bad)).toThrow(RangeError)
})

it.each([0.5, NaN, Infinity, -1])('rejects invalid coordinate %s without poisoning state', (bad) => {
  const state = [1, 2]
  expect(() => bumpDelta(bad, 3)).toThrow(RangeError)
  expect(() => applyGCounterDelta(state, { kind: 'gcounter.bump', replica: bad, tally: 3 })).toThrow(RangeError)
  expect(state).toEqual([1, 2])
})

it.each([0.5, NaN, Infinity, -1])('rejects invalid tally %s without poisoning state', (bad) => {
  const state = [1, 2]
  expect(() => bumpDelta(0, bad)).toThrow(RangeError)
  expect(() => applyGCounterDelta(state, { kind: 'gcounter.bump', replica: 0, tally: bad })).toThrow(RangeError)
  expect(state).toEqual([1, 2])
})

it('refuses an inexact total in either order while preserving the safe boundary', () => {
  for (const state of [[Number.MAX_SAFE_INTEGER, 2], [2, Number.MAX_SAFE_INTEGER]]) {
    expect(() => readGCounter(state)).toThrow(RangeError)
    expect(() => readGCounter(mergeGCounter(state, state))).toThrow(RangeError)
  }
  expect(readGCounter([Number.MAX_SAFE_INTEGER - 2, 2])).toBe(Number.MAX_SAFE_INTEGER)
  expect(readGCounter([])).toBe(0)
})

it.each([Number.MAX_SAFE_INTEGER + 1, -1, 0.5, NaN, Infinity])('refuses unsafe read coordinate %s', (bad) => {
  expect(() => readGCounter([bad])).toThrow(RangeError)
})
