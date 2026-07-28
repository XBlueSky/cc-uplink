import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const REQUIRED = ['id', 'name', 'version', 'description', 'tagline', 'intro']
const TOOL_COUNT = 6

/**
 * @param {string | object} input
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

  const skill = plugin.skills?.[0] ?? null

  return {
    name: plugin.name,
    tagline: plugin.tagline.trim(),
    intro: plugin.intro.trim(),
    description: plugin.description.trim(),
    version: plugin.version,
    repository: plugin.repository ?? plugin.homepage ?? null,
    tools,
    setup: (server?.setup ?? []).map((s) => ({ text: s.text, cmd: s.cmd ?? null })),
    tips: (plugin.tips ?? []).map((t) => t.text),
    traps: (plugin.traps ?? []).map((t) => t.text),
    skill: skill && {
      name: skill.name,
      trigger: skill.trigger ?? null,
      examples: skill.examples ?? [],
    },
  }
}

const DEFAULT_PATH = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../../../.cc-marketspec/dist/manifest.json',
)

export function loadManifest(path = DEFAULT_PATH) {
  return parseManifest(readFileSync(path, 'utf8'))
}
