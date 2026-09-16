import { describe, expect, it } from 'vitest'
import { applyGCounterDelta, bottomGCounter, bumpDelta, mergeGCounter, readGCounter } from './gcounter'
import { readPNCounter } from './pncounter'

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

it.each([Number.MAX_SAFE_INTEGER + 1, -1, 0.5, NaN, Infinity])('refuses invalid PN coordinate %s on either side before cancellation', (bad) => {
  const message = 'G-Counter read requires safe nonnegative integer coordinates'
  for (const state of [
    { p: [bad], n: [0] },
    { p: [0], n: [bad] },
    { p: [bad], n: [Number.MAX_SAFE_INTEGER] },
    { p: [Number.MAX_SAFE_INTEGER], n: [bad] },
    { p: [bad], n: [bad] },
  ]) {
    expect(() => readPNCounter(state)).toThrow(RangeError)
    expect(() => readPNCounter(state)).toThrow(message)
  }
})

it('cancels four merged replica totals before checking the PN read range', () => {
  const total = [0, 1, 2, 3].reduce((state, replica) => {
    const contribution = applyGCounterDelta(bottomGCounter(4), bumpDelta(replica, 2 ** 51))
    return mergeGCounter(state, contribution)
  }, bottomGCounter(4))
  expect(() => readGCounter(total)).toThrow(RangeError)
  expect(readPNCounter({ p: total, n: [...total] })).toBe(0)
  expect(() => readPNCounter({ p: total, n: bottomGCounter(4) })).toThrow(RangeError)
  expect(() => readPNCounter({ p: bottomGCounter(4), n: total })).toThrow(RangeError)
})

it('preserves exact safe PN differences across overflowing component sums', () => {
  const p = [Number.MAX_SAFE_INTEGER, 2]
  const n = [Number.MAX_SAFE_INTEGER, 1]
  expect(readPNCounter({ p, n })).toBe(1)
  expect(readPNCounter({ p: n, n: p })).toBe(-1)
  expect(readPNCounter({ p: [Number.MAX_SAFE_INTEGER, 1], n: [1] })).toBe(Number.MAX_SAFE_INTEGER)
  expect(readPNCounter({ p: [1], n: [Number.MAX_SAFE_INTEGER, 1] })).toBe(-Number.MAX_SAFE_INTEGER)
  expect(readPNCounter({ p: [7, 2], n: [3, 1] })).toBe(5)
  expect(readPNCounter({ p: [], n: [] })).toBe(0)
})
