import { test } from 'node:test'
import assert from 'node:assert/strict'

import { fetchLatestRelease, resolveRelease } from '../src/lib/release.mjs'

const payload = {
  tag_name: 'v0.1.0',
  html_url: 'https://github.com/XBlueSky/cc-uplink/releases/tag/v0.1.0',
  assets: [{
    name: 'cc-uplink-x86_64-unknown-linux-musl.tar.gz',
    browser_download_url: 'https://example.test/musl.tar.gz',
  }],
}

test('maps a successful response', async () => {
  const release = await fetchLatestRelease({
    fetchImpl: async () => ({ ok: true, json: async () => payload }),
  })
  assert.equal(release.tag, 'v0.1.0')
  assert.equal(release.assets.length, 1)
  assert.equal(release.assets[0].url, 'https://example.test/musl.tar.gz')
})

test('returns null on a non-ok response', async () => {
  const release = await fetchLatestRelease({
    fetchImpl: async () => ({ ok: false, json: async () => ({}) }),
  })
  assert.equal(release, null)
})

test('returns null when fetch throws', async () => {
  const release = await fetchLatestRelease({
    fetchImpl: async () => { throw new Error('offline') },
  })
  assert.equal(release, null)
})

test('returns null when the payload has no tag', async () => {
  const release = await fetchLatestRelease({
    fetchImpl: async () => ({ ok: true, json: async () => ({}) }),
  })
  assert.equal(release, null)
})

test('resolveRelease passes a real release through', () => {
  const real = { tag: 'v9.9.9', url: 'https://example.test/tag', assets: [] }
  assert.equal(resolveRelease(real, '0.1.0'), real)
})

test('resolveRelease falls back to the manifest version', () => {
  const release = resolveRelease(null, '0.1.0')
  assert.equal(release.tag, 'v0.1.0')
  assert.equal(release.url, 'https://github.com/XBlueSky/cc-uplink/releases/latest')
  assert.deepEqual(release.assets, [])
})
