import assert from 'node:assert/strict'
import test from 'node:test'
import { MAC_DOWNLOADS, resolveMacDownloadUrl } from '../src/lib/mac-download.ts'
import type { MacClientHints } from '../src/lib/mac-download.ts'

function macHints(architecture?: string, bitness?: string): MacClientHints {
  return { platform: 'macOS', getHighEntropyValues: async () => ({ architecture, bitness }) }
}

test('64-bit Intel Macs download the Intel release', async () => {
  assert.equal(await resolveMacDownloadUrl(macHints('x86', '64')), MAC_DOWNLOADS.intel)
})

test('Apple silicon Macs download the Apple silicon release', async () => {
  assert.equal(await resolveMacDownloadUrl(macHints('arm', '64')), MAC_DOWNLOADS.appleSilicon)
})

test('only requests architecture and bitness, preserving the API receiver', async () => {
  const hints: MacClientHints = {
    platform: 'macOS',
    async getHighEntropyValues(requested) {
      assert.equal(this, hints)
      assert.deepEqual(requested, ['architecture', 'bitness'])
      return { architecture: 'x86', bitness: '64' }
    },
  }
  assert.equal(await resolveMacDownloadUrl(hints), MAC_DOWNLOADS.intel)
})

test('missing or partial APIs use Apple silicon', async () => {
  for (const hints of [undefined, {}, { platform: 'macOS' }]) {
    assert.equal(await resolveMacDownloadUrl(hints), MAC_DOWNLOADS.appleSilicon)
  }
})

test('non-Mac visitors use Apple silicon without requesting hardware hints', async () => {
  for (const platform of ['Windows', 'Linux', 'Android', 'iOS', undefined]) {
    let requested = false
    assert.equal(await resolveMacDownloadUrl({
      platform,
      getHighEntropyValues: async () => { requested = true; return { architecture: 'x86', bitness: '64' } },
    }), MAC_DOWNLOADS.appleSilicon)
    assert.equal(requested, false)
  }
})

test('unknown, withheld, or incomplete architecture values use Apple silicon', async () => {
  for (const hints of [macHints(), macHints('', ''), macHints('unknown', '64'), macHints('x86'), macHints('x86', '32')]) {
    assert.equal(await resolveMacDownloadUrl(hints), MAC_DOWNLOADS.appleSilicon)
  }
})

test('rejected and synchronously failing hint requests use Apple silicon', async () => {
  for (const getHighEntropyValues of [
    async () => { throw new Error('blocked by browser policy') },
    () => { throw new Error('API unavailable') },
  ]) {
    assert.equal(await resolveMacDownloadUrl({ platform: 'macOS', getHighEntropyValues }), MAC_DOWNLOADS.appleSilicon)
  }
})

test('a stalled hint request falls back after one second', async (context) => {
  context.mock.timers.enable({ apis: ['setTimeout'] })
  const selection = resolveMacDownloadUrl({
    platform: 'macOS',
    getHighEntropyValues: () => new Promise(() => {}),
  })
  context.mock.timers.tick(1000)
  assert.equal(await selection, MAC_DOWNLOADS.appleSilicon)
})
