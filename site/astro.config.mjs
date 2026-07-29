import { defineConfig } from 'astro/config'

export default defineConfig({
  site: 'https://cc-uplink.pages.dev',
  build: { format: 'directory' },
  markdown: {
    // Shiki's default `github-dark` theme paints its own inline
    // background/foreground colours on every <pre>, which both override
    // Doc.astro's `--bg-raised` background (inline styles always win over
    // an external stylesheet rule, regardless of specificity) and — for the
    // comment token specifically — measure at 3.05:1 against that theme's
    // own background, under the 4.5:1 AA floor this project's design spec
    // requires. `'css-variables'` makes Shiki emit `var(--astro-code-*)`
    // instead of hardcoded hex, so the actual colours come from this site's
    // own palette (defined in src/styles/tokens.css) and are measured
    // against the background docs code actually renders on. See
    // tokens.css for the values and site/test/contrast.test.mjs for the
    // measurements.
    shikiConfig: { theme: 'css-variables' },
  },
})
