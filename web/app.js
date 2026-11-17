// Wires a game to something that can show it.
//
// The game is either in the page (Rust compiled to WebAssembly, one
// player) or on a server (a WebSocket, many players). Nothing here knows
// about WebAssembly or the DOM, so it runs under `node --test` with a
// fake game, a fake socket and a fake view.

/**
 * A game as the page sees it. `eq-web`'s `Shore` is one.
 * @typedef {object} Game
 * @property {() => string} look      The opening description.
 * @property {(line: string) => string} input   Handles a line; returns a JSON array of lines to show.
 * @property {() => string} scene     What to draw, as JSON.
 */

/**
 * Something that can show the game.
 * @typedef {object} View
 * @property {(scene: object, lines: string[]) => void} show
 */

/** How many bots share the shoreline unless the address says otherwise. */
export const DEFAULT_BOTS = 3

/** The most bots the address can ask for. */
export const MOST_BOTS = 24

/** The protocol version this page speaks. Must match `eq-protocol`'s `VERSION`. */
export const PROTOCOL_VERSION = 1

/**
 * Reads the game's settings from the page address: `?seed=7&bots=0`, or
 * `?server=ws://127.0.0.1:4000` to play on a server.
 * Anything missing or malformed falls back to seed 1, three bots and no server.
 * @param {URLSearchParams} query
 * @returns {{ seed: number, bots: number, server: string | null }}
 */
export function settings(query) {
  const whole = (name) => (/^\d+$/.test(query.get(name) ?? '') ? Number(query.get(name)) : null)
  const server = query.get('server') ?? ''
  return {
    seed: whole('seed') ?? 1,
    bots: Math.min(whole('bots') ?? DEFAULT_BOTS, MOST_BOTS),
    server: /^wss?:\/\/[^\s]+$/.test(server) ? server : null,
  }
}

/**
 * Starts a game and draws its first frame.
 * @param {Game} game
 * @param {View} view
 * @returns {(line: string) => void} Sends one line to the game and redraws.
 */
export function startGame(game, view) {
  const lines = [game.look()]
  const draw = () => view.show(JSON.parse(game.scene()), [...lines])

  draw()
  return function send(line) {
    const typed = line.trim()
    if (!typed) return
    lines.push(`> ${typed}`, ...JSON.parse(game.input(typed)))
    draw()
  }
}

/**
 * A WebSocket, or anything shaped like one.
 * @typedef {object} Socket
 * @property {(data: string) => void} send
 * @property {(type: string, listener: (event: { data?: string }) => void) => void} addEventListener
 */

/**
 * Plays on a server. The server holds the game and does the narrating;
 * this sends the lines the player types and shows what comes back,
 * whenever it comes.
 * @param {Socket} socket  A socket that has not opened yet.
 * @param {View} view
 * @param {{ name: string, key: string }} login
 * @param {{ onRefused?: (reason: string) => void, onClosed?: () => void }} [hooks]
 * @returns {(line: string) => void} Sends one line to the server.
 */
export function startOnline(socket, view, login, hooks = {}) {
  const lines = []
  let scene = null
  let open = false
  // Nothing can be drawn until the server has sent a first scene.
  const draw = () => scene && view.show(scene, [...lines])
  const say = (message) => socket.send(JSON.stringify(message))

  socket.addEventListener('open', () => {
    open = true
    say({ type: 'hello', version: PROTOCOL_VERSION, name: login.name, key: login.key, scenes: true })
  })
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data)
    if (message.type === 'welcome') lines.push(message.new ? `Welcome, ${message.name}.` : `Welcome back, ${message.name}.`)
    if (message.type === 'lines') lines.push(...message.lines)
    if (message.type === 'scene') scene = message.scene
    if (message.type === 'refused') {
      lines.push(message.reason)
      hooks.onRefused?.(message.reason)
    }
    draw()
  })
  socket.addEventListener('close', () => {
    open = false
    lines.push('The connection has closed. Your character stays in the world.')
    draw()
    hooks.onClosed?.()
  })

  return function send(line) {
    const typed = line.trim()
    if (!typed || !open) return
    lines.push(`> ${typed}`)
    say({ type: 'line', text: typed })
    draw()
  }
}

/**
 * The page's four components as one view.
 * @param {ParentNode} root  Where to find `eq-map`, `eq-status`, `eq-actions` and `eq-log`.
 * @param {{ online?: boolean }} [options]  `online` when the game is on a server, where time is not the player's to spend.
 * @returns {View & { onCommand: (send: (line: string) => void) => void }}
 */
export function pageView(root, options = {}) {
  const map = root.querySelector('eq-map')
  const status = root.querySelector('eq-status')
  const actions = root.querySelector('eq-actions')
  const log = root.querySelector('eq-log')

  return {
    show(scene, lines) {
      Object.assign(map, { places: scene.places, links: scene.links, exits: scene.exits })
      Object.assign(status, {
        clock: scene.clock,
        tide: scene.tide,
        vitality: scene.vitality,
        health: scene.health,
        you: scene.you,
        company: scene.company,
      })
      Object.assign(actions, {
        exits: scene.exits,
        canGather: scene.you.can_gather,
        canTake: scene.you.can_take,
        targets: scene.company.filter((person) => person.can_attack),
        sleepers: scene.company.filter((person) => person.can_rob),
        foes: scene.places.find((place) => place.here)?.creatures ?? [],
        body: scene.you.body,
        forgotten: scene.you.forgotten,
        asleep: scene.you.asleep,
        canRite: scene.you.can_rite,
        riteWaiting: scene.you.rite_waiting,
        online: Boolean(options.online),
      })
      log.lines = lines
    },
    onCommand(send) {
      map.addEventListener('travel', (event) => send(event.detail))
      actions.addEventListener('command', (event) => send(event.detail))
    },
  }
}
