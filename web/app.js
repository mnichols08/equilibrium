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
      })
      Object.assign(actions, { exits: scene.exits, canGather: scene.you.can_gather })
      log.lines = lines
    },
    onCommand(send) {
      map.addEventListener('travel', (event) => send(event.detail))
      actions.addEventListener('command', (event) => send(event.detail))
    },
  }
}
