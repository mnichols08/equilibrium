// A static file server for the browser tests, with one trick.
//
// Headless Chrome's `--dump-dom` prints the page as soon as it has
// loaded. A test that talks to a real server over a WebSocket needs real
// time to pass first. So the test page embeds `/hold`, which does not
// answer until the page asks for `/release`: the page is not "loaded"
// until the test says it is done.
//
// Usage: node serve.js <directory> <port>

import { createReadStream, existsSync, statSync } from 'node:fs'
import { createServer } from 'node:http'
import { extname, join, normalize } from 'node:path'

const [root, port] = [process.argv[2], Number(process.argv[3])]
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.json': 'application/json' }

/** The longest `/hold` waits, so a broken test cannot hang the build. */
const LONGEST_HOLD_MS = 60_000

let held = []
const release = () => {
  held.forEach((response) => response.end('<!doctype html><title>released</title>'))
  held = []
}

createServer((request, response) => {
  const path = new URL(request.url, 'http://localhost').pathname
  if (path === '/hold') {
    response.writeHead(200, { 'content-type': 'text/html' })
    held.push(response)
    setTimeout(release, LONGEST_HOLD_MS)
    return
  }
  if (path === '/release') {
    release()
    response.end('ok')
    return
  }
  // Keep requests inside the directory being served.
  const file = join(root, normalize(path).replace(/^([/\\]|\.\.)+/, ''))
  if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) {
    response.writeHead(404).end('not found')
    return
  }
  response.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' })
  createReadStream(file).pipe(response)
}).listen(port, '127.0.0.1')
