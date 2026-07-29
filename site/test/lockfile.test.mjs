import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

/**
 * `npm ci` fetches each dependency from its lockfile `resolved` URL verbatim
 * — it ignores whatever registry is configured in `.npmrc` for that purpose.
 * If `package-lock.json` ever gets regenerated against a private/internal
 * registry mirror again (e.g. a laptop with a machine-wide ~/.npmrc pointing
 * at one), every `resolved` entry would silently repin to a host that
 * doesn't resolve on GitHub Actions or the Cloudflare Pages builder — a
 * green install here, a broken install everywhere else. `site/.npmrc` and
 * the repo-root `.npmrc` are the one-time fix; this test is the durable one,
 * so a regression fails the build loudly instead of waiting to be
 * rediscovered the same way this one was.
 */
const LOCKFILE = new URL('../package-lock.json', import.meta.url).pathname
const EXPECTED_HOST = 'registry.npmjs.org'

test('every package-lock.json "resolved" URL points at the public npm registry', () => {
  const lock = JSON.parse(readFileSync(LOCKFILE, 'utf8'))
  const packages = lock.packages ?? {}

  const offenders = Object.entries(packages)
    .filter(([, pkg]) => typeof pkg.resolved === 'string')
    .map(([name, pkg]) => [name, new URL(pkg.resolved).host])
    .filter(([, host]) => host !== EXPECTED_HOST)
    .map(([name, host]) => `${name || '(root)'} -> ${host}`)

  assert.deepEqual(
    offenders,
    [],
    `package-lock.json pins dependencies to a non-public registry, which ` +
      `\`npm ci\` will fetch verbatim and which will not resolve off this ` +
      `machine:\n${offenders.join('\n')}`,
  )
})
