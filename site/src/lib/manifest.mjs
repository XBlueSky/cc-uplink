import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const REQUIRED = ['id', 'name', 'version', 'description', 'tagline', 'intro']
const TOOL_COUNT = 6

/**
 * @typedef {object} Manifest
 * @property {string} name
 * @property {string} tagline
 * @property {string} intro
 * @property {string} description
 * @property {string} version
 * @property {string | null} repository
 * @property {string | null} marketplace
 * @property {string[]} tools
 * @property {{text: string, cmd: string | null}[]} setup
 * @property {string[]} tips
 * @property {string[]} traps
 * @property {{name: string, trigger: string | null, examples: string[]} | null} skill
 */

/**
 * Flattens an array of `{text}` entries into `string[]`, throwing if any
 * entry's `text` is missing or blank. Absent input tolerantly yields `[]`.
 *
 * @param {Array<{text?: unknown}>} items
 * @param {string} name used in the thrown error to name the offending array
 * @returns {string[]}
 */
function flattenTextList(items, name) {
  return items.map((item, i) => {
    const text = item?.text
    if (typeof text !== 'string' || text.trim() === '') {
      throw new Error(`manifest ${name}[${i}].text is missing or empty`)
    }
    return text
  })
}

/**
 * @param {string | object} input
 * @returns {Manifest}
 */
export function parseManifest(input) {
  const data = typeof input === 'string' ? JSON.parse(input) : input

  const plugin = data?.plugins?.[0]
  if (!plugin) {
    throw new Error('manifest has no plugins — run `npm run sync` first')
  }

  for (const field of REQUIRED) {
    const value = plugin[field]
    if (typeof value !== 'string' || value.trim() === '') {
      throw new Error(`manifest plugin.${field} is missing or empty`)
    }
  }

  const server = plugin.mcp?.[0]
  const tools = server?.provides ?? []
  if (tools.length !== TOOL_COUNT) {
    throw new Error(
      `the site claims six fixed tools but the manifest declares ${tools.length}`,
    )
  }
  for (const [i, tool] of tools.entries()) {
    if (typeof tool !== 'string' || tool.trim() === '') {
      throw new Error(`manifest tools[${i}] is missing or empty`)
    }
  }

  const skill = plugin.skills?.[0] ?? null

  return {
    name: plugin.name,
    tagline: plugin.tagline.trim(),
    intro: plugin.intro.trim(),
    description: plugin.description.trim(),
    version: plugin.version,
    repository: plugin.repository ?? plugin.homepage ?? null,
    // The install command's `plugin@marketplace` half needs the *marketplace*
    // entry's own name, not the plugin's — they happen to be identical in
    // this repo today, but reading `data.marketplace.name` explicitly (top
    // level of the manifest, a sibling of `plugins`, not nested under it)
    // means a future repo where they diverge still renders a working
    // install command instead of a silently-identical-looking wrong one.
    marketplace: data?.marketplace?.name ?? null,
    tools,
    setup: (server?.setup ?? []).map((s) => ({ text: s.text, cmd: s.cmd ?? null })),
    tips: flattenTextList(plugin.tips ?? [], 'tips'),
    traps: flattenTextList(plugin.traps ?? [], 'traps'),
    skill: skill && {
      name: skill.name,
      trigger: skill.trigger ?? null,
      examples: skill.examples ?? [],
    },
  }
}

/**
 * The on-disk location `npm run sync` writes the marketspec manifest to,
 * for Node-side callers (tests, scripts) that load it directly — contexts
 * where this module is imported from its real on-disk location, so a path
 * derived from its own `import.meta.url` is trustworthy.
 *
 * Not a default for `loadManifest`: a page bundled by Astro's build can be
 * relocated into a hashed chunk under `dist/.prerender/chunks/`, which
 * moves *that module's* `import.meta.url` but not this constant's — so a
 * bundled caller silently inheriting this path would be wrong. Bundled
 * pages must reach the manifest via a static JSON import instead (see
 * `src/pages/index.astro`), which Vite resolves and inlines at build time.
 */
export const MANIFEST_PATH = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../../../.cc-marketspec/dist/manifest.json',
)

/**
 * @param {string} path required — there is no safe default, see `MANIFEST_PATH`'s doc comment
 * @returns {Manifest}
 */
export function loadManifest(path) {
  if (!path) {
    throw new Error(
      'loadManifest requires an explicit path — pass `MANIFEST_PATH` from Node-side callers, or use a static import with parseManifest for bundled pages',
    )
  }
  let raw
  try {
    raw = readFileSync(path, 'utf8')
  } catch (cause) {
    throw new Error(
      `could not read manifest at ${path} — run \`npm run sync\` first`,
      { cause },
    )
  }
  return parseManifest(raw)
}
