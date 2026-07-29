const REPO = 'XBlueSky/cc-uplink'

/**
 * @returns {Promise<{tag: string, url: string, assets: {name: string, url: string}[]} | null>}
 */
export async function fetchLatestRelease({ fetchImpl = fetch, repo = REPO } = {}) {
  try {
    const response = await fetchImpl(
      `https://api.github.com/repos/${repo}/releases/latest`,
      { headers: { accept: 'application/vnd.github+json', 'user-agent': 'cc-uplink-site' } },
    )
    if (!response.ok) return null

    const data = await response.json()
    if (!data?.tag_name) return null

    return {
      tag: data.tag_name,
      url: data.html_url,
      assets: (data.assets ?? []).map((asset) => ({
        name: asset.name,
        url: asset.browser_download_url,
      })),
    }
  } catch {
    return null
  }
}

export function resolveRelease(fetched, manifestVersion, repo = REPO) {
  if (fetched) return fetched
  return {
    tag: `v${manifestVersion}`,
    url: `https://github.com/${repo}/releases/latest`,
    assets: [],
  }
}
