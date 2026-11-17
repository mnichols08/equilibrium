// Wires a game to something that can show it.
//
// Nothing here knows about WebAssembly or the DOM, so it runs under
// `node --test` with a fake game and a fake view.

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

/**
 * Reads the game's settings from the page address: `?seed=7&bots=0`.
 * Anything missing or malformed falls back to seed 1 and three bots.
 * @param {URLSearchParams} query
 * @returns {{ seed: number, bots: number }}
 */
export function settings(query) {
  const whole = (name) => (/^\d+$/.test(query.get(name) ?? '') ? Number(query.get(name)) : null)
  return {
    seed: whole('seed') ?? 1,
    bots: Math.min(whole('bots') ?? DEFAULT_BOTS, MOST_BOTS),
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
 * The page's four components as one view.
 * @param {ParentNode} root  Where to find `eq-map`, `eq-status`, `eq-actions` and `eq-log`.
 * @returns {View & { onCommand: (send: (line: string) => void) => void }}
 */
export function pageView(root) {
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
        body: scene.you.body,
        forgotten: scene.you.forgotten,
      })
      log.lines = lines
    },
    onCommand(send) {
      map.addEventListener('travel', (event) => send(event.detail))
      actions.addEventListener('command', (event) => send(event.detail))
    },
  }
}
