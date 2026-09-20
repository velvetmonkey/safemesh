import { describe, expect, it, vi } from 'vitest'
import { addDelta, applyORSetDelta, bottomORSet, mergeORSet, observedTokens, readORSet, removeDelta } from './orset'

describe('OR-Set mirror', () => {
  it.each(['de', 'sv'])('reads visible elements in ordinal order under %s collation', (locale) => {
    const state = [addDelta('ä', 'p0-1'), addDelta('z', 'p0-2')].reduce(
      applyORSetDelta,
      bottomORSet(),
    )
    const collator = new Intl.Collator(locale)
    expect(collator.resolvedOptions().locale).toBe(locale)
    // German puts ä before z; Swedish puts it after z. Emulate each default locale.
    expect(Math.sign(collator.compare('ä', 'z'))).toBe(locale === 'de' ? -1 : 1)
    const comparison = vi.spyOn(String.prototype, 'localeCompare').mockImplementation(function (this: string, other) {
      return collator.compare(String(this), other)
    })
    try {
      expect(readORSet(state)).toEqual(['z', 'ä'])
    } finally {
      comparison.mockRestore()
    }
  })

  it('keeps a concurrent fresh add visible after an observed-token remove', () => {
    const first = addDelta('radio', 'p0-1')
    const concurrent = addDelta('radio', 'p1-2')
    const removerView = applyORSetDelta(bottomORSet(), first)
    const remove = removeDelta(observedTokens(removerView, 'radio'))

    const merged = [first, remove, concurrent].reduce(applyORSetDelta, bottomORSet())

    expect(readORSet(merged)).toEqual(['radio'])
    expect(merged.tombstones).toEqual({ 'p0-1': true })
  })

  it('removes an element when all observed add tokens are tombstoned', () => {
    const state = [addDelta('water', 'p0-1'), removeDelta(['p0-1'])].reduce(
      applyORSetDelta,
      bottomORSet(),
    )

    expect(readORSet(state)).toEqual([])
  })
})

it.each(['__proto__', 'constructor', 'toString'])('preserves and removes prototype-named token %s', (token) => {
  const added = applyORSetDelta(bottomORSet(), addDelta('water', token))
  expect(readORSet(added)).toEqual(['water'])
  expect(observedTokens(added, 'water')).toEqual([token])
  const removed = applyORSetDelta(bottomORSet(), removeDelta([token]))
  expect(Object.hasOwn(removed.tombstones, token)).toBe(true)
  expect(readORSet(mergeORSet(added, removed))).toEqual([])
  expect(readORSet(mergeORSet(removed, added))).toEqual([])
  expect(readORSet(applyORSetDelta(removed, addDelta('water', token)))).toEqual([])
})

it.each(['token', '__proto__'])('refuses conflicting values for token %s in either order', (token) => {
  const radio = applyORSetDelta(bottomORSet(), addDelta('radio', token))
  const water = applyORSetDelta(bottomORSet(), addDelta('water', token))
  expect(() => mergeORSet(radio, water)).toThrow(/conflicting/)
  expect(() => mergeORSet(water, radio)).toThrow(/conflicting/)
  expect(() => applyORSetDelta(radio, addDelta('water', token))).toThrow(/conflicting/)
  expect(() => applyORSetDelta(water, addDelta('radio', token))).toThrow(/conflicting/)
  const removed = applyORSetDelta(radio, removeDelta([token]))
  expect(() => mergeORSet(removed, water)).toThrow(/conflicting/)
  expect(() => mergeORSet(water, removed)).toThrow(/conflicting/)
  expect(readORSet(radio)).toEqual(['radio'])
  expect(readORSet(water)).toEqual(['water'])
  expect(mergeORSet(radio, radio)).toEqual(radio)
  expect(applyORSetDelta(radio, addDelta('radio', token))).toEqual(radio)
  const fresh = applyORSetDelta(bottomORSet(), addDelta('water', 'fresh'))
  const merged = mergeORSet(radio, fresh)
  expect(mergeORSet(fresh, radio)).toEqual(merged)
  expect(mergeORSet(merged, fresh)).toEqual(merged)
  expect(readORSet(merged)).toEqual(['radio', 'water'])
})
