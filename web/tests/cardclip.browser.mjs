import assert from 'node:assert/strict'
import { after, before, test } from 'node:test'
import { chromium } from 'playwright'
import { createServer } from 'vite'

let server, browser, url
before(async () => {
  server = await createServer({ server: { host: '127.0.0.1', port: 0 } })
  await server.listen()
  url = server.resolvedUrls.local[0]
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined })
})
after(async () => { await browser?.close(); await server?.close() })

// Real controls, real WASM, real layout: no injected simulation or mocked boxes.
for (const width of [1920, 1440, 1280, 1024, 821, 820, 760, 520, 360]) {
  for (const count of [3, 4, 5, 6]) {
    test(`${width}px / ${count} replicas: all supply text remains reachable`, async () => {
      const page = await browser.newPage({ viewport: { width, height: 1000 }, reducedMotion: 'reduce' })
      try {
        page.setDefaultTimeout(10000)
        await page.goto(url)
        const cue = page.locator('.stage-cue')
        const stage = page.locator('.stage')
        assert.ok((await cue.boundingBox()).y + (await cue.boundingBox()).height <= (await stage.boundingBox()).y)
        await page.getByRole('button', { name: 'Open advanced lab' }).click()
        await page.locator('.tuning-bank select').selectOption(String(count))
        const cards = page.locator('.replica')
        const other = cards.nth(1)
        await page.mouse.move(0, 0)
        const idleBorder = await other.evaluate(el => getComputedStyle(el).borderColor)
        await other.hover()
        assert.notEqual(await other.evaluate(el => getComputedStyle(el).borderColor), idleBorder)
        await page.mouse.move(0, 0)
        await page.keyboard.press('Tab')
        await other.focus()
        assert.equal(await other.evaluate(el => el.matches(':focus-visible')), true)
        assert.notEqual(await other.evaluate(el => getComputedStyle(el).borderColor), idleBorder)
        assert.equal(await other.getAttribute('aria-pressed'), 'false')
        await cards.first().click()
        const supplies = await page.getByLabel('Record element').locator('option').allTextContents()
        const measure = () => stage.evaluate(el => {
          const s = el.getBoundingClientRect()
          return [...el.querySelectorAll('.replica')].map(card => {
            const r = card.getBoundingClientRect()
            return { x: r.x - s.x, y: r.y - s.y, width: r.width, height: r.height }
          })
        })
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
        const assertDesktopFit = async () => {
          if (width < 1024) return
          await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
          const sizes = await page.evaluate(() => {
            const viewport = document.querySelector('.stage-viewport')
            return {
              pageClient: document.documentElement.clientWidth,
              pageScroll: document.documentElement.scrollWidth,
              diagramClient: viewport.clientWidth,
              diagramScroll: viewport.scrollWidth,
            }
          })
          assert.equal(sizes.pageScroll, sizes.pageClient, 'no horizontal page overflow on desktop')
          assert.equal(sizes.diagramScroll, sizes.diagramClient, 'no horizontal diagram scrollbar on desktop')
        }
        await assertDesktopFit()
        const empty = await measure()
        for (const supply of supplies) {
          await page.getByLabel('Record element').selectOption(supply)
          await page.getByRole('button', { name: 'Add', exact: true }).click()
          await assertDesktopFit()
        }
        await page.getByRole('button', { name: 'Heal', exact: true }).click()
        // Wait for state delivery and the ResizeObserver layout pass.
        await page.waitForFunction(n => [...document.querySelectorAll('.replica')].every(c => c.textContent.includes(n)), supplies.at(-1))
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
        await assertDesktopFit()
        const populated = await measure()
        for (let i = 0; i < count; i++) {
          const card = cards.nth(i)
          await card.scrollIntoViewIfNeeded()
          await card.click()
          assert.equal(await card.getAttribute('aria-pressed'), 'true')
          assert.equal(await cards.locator('..').locator('.replica[aria-pressed="true"]').count(), 1)
          assert.ok((await card.ariaSnapshot()).includes('pressed'))
          for (const supply of supplies) assert.ok((await card.textContent()).includes(supply))
          // Text rectangles must be inside both the card and every clipping
          // ancestor after scrolling; hit testing catches opaque overlays.
          const errors = await card.evaluate(el => {
            const errors = [], card = el.getBoundingClientRect()
            const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT)
            while (walker.nextNode()) {
              const node = walker.currentNode
              if (!node.textContent.trim() || node.parentElement.closest('.visually-hidden') || getComputedStyle(node.parentElement).display === 'none') continue
              const range = document.createRange(); range.selectNodeContents(node)
              for (const r of range.getClientRects()) {
                if (!r.width || !r.height) continue
                if (r.left < card.left || r.right > card.right || r.top < card.top || r.bottom > card.bottom) errors.push(`outside card: ${node.textContent}`)
                for (let p = el.parentElement; p; p = p.parentElement) {
                  if (/(auto|scroll|hidden|clip)/.test(getComputedStyle(p).overflow)) {
                    const b = p.getBoundingClientRect()
                    if (r.left < b.left - 1 || r.right > b.right + 1 || r.top < b.top - 1 || r.bottom > b.bottom + 1) errors.push(`clipped: ${node.textContent}`)
                  }
                }
                const hit = document.elementFromPoint((r.left + r.right) / 2, (r.top + r.bottom) / 2)
                if (!el.contains(hit)) errors.push(`covered: ${node.textContent}`)
              }
            }
            return errors
          })
          assert.deepEqual(errors, [])
          assert.equal(await card.evaluate(el => getComputedStyle(el).outlineStyle), 'solid')
        }
        const overlaps = await stage.evaluate(el => {
          const cards = [...el.querySelectorAll('.replica, .convergence-core')]
          return cards.flatMap((a, i) => cards.slice(i + 1).filter(b => {
            const x = a.getBoundingClientRect(), y = b.getBoundingClientRect()
            return x.left < y.right && y.left < x.right && x.top < y.bottom && y.top < x.bottom
          }).map(b => `${a.className} / ${b.className}`))
        })
        assert.deepEqual(overlaps, [])
        await page.getByRole('button', { name: 'Reset', exact: true }).click()
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
        assert.deepEqual(await measure(), empty, 'reset restores empty card geometry')
        console.log(JSON.stringify({ width, count, empty, populated }))
      } finally { await page.close() }
    })
  }
}
