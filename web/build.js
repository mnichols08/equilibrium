// Builds the site into web/dist:
//
//   dist/index.html, main.js, app.js   the page
//   dist/components/                   the Zubra components as browser modules
//   dist/pkg/                          the game as WebAssembly (put there by wasm-bindgen)
//
// Usage: node build.js

import { execFileSync } from 'node:child_process'
import { cpSync, mkdirSync, rmSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const here = (path) => fileURLToPath(new URL(path, import.meta.url))

mkdirSync(here('dist'), { recursive: true })
for (const file of ['index.html', 'main.js', 'app.js']) {
  cpSync(here(file), here(`dist/${file}`))
}

rmSync(here('dist/components'), { recursive: true, force: true })
execFileSync('npx', ['zubra', 'build', '--embed', 'components', '--out', 'dist/components', '--name', 'eq-components'], {
  cwd: here('.'),
  stdio: 'inherit',
})
// The embed build also writes installers for other platforms; the site only needs the modules.
for (const extra of ['wordpress', 'shopify', 'html', 'README.md']) {
  rmSync(here(`dist/components/${extra}`), { recursive: true, force: true })
}
