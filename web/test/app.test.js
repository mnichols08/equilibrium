import assert from 'node:assert/strict'
import { test } from 'node:test'

import { DEFAULT_BOTS, MOST_BOTS, settings, startGame } from '../app.js'

/** A game that records what it is sent and counts its turns. */
function fakeGame() {
  const sent = []
  return {
    sent,
    look: () => 'You are at camp.',
    input(line) {
      sent.push(line)
      return JSON.stringify([`You ${line}.`])
    },
    scene: () => JSON.stringify({ turn: sent.length }),
  }
}

/** A view that keeps every frame it is shown. */
function fakeView() {
  const frames = []
  return { frames, show: (scene, lines) => frames.push({ scene, lines }) }
}

test('the first frame shows the opening description', () => {
  const view = fakeView()
  startGame(fakeGame(), view)
  assert.deepEqual(view.frames, [{ scene: { turn: 0 }, lines: ['You are at camp.'] }])
})

test('sending a line logs it, logs the reply and redraws', () => {
  const game = fakeGame()
  const view = fakeView()
  const send = startGame(game, view)

  send('gather')

  assert.deepEqual(game.sent, ['gather'])
  assert.deepEqual(view.frames.at(-1), {
    scene: { turn: 1 },
    lines: ['You are at camp.', '> gather', 'You gather.'],
  })
})

test('blank lines are ignored and typed lines are trimmed', () => {
  const game = fakeGame()
  const view = fakeView()
  const send = startGame(game, view)

  send('   ')
  send('  north ')

  assert.deepEqual(game.sent, ['north'])
  assert.equal(view.frames.length, 2)
})

test('each frame gets its own copy of the lines', () => {
  const view = fakeView()
  const send = startGame(fakeGame(), view)
  send('wait')
  assert.equal(view.frames[0].lines.length, 1, 'an earlier frame must not grow when a later one is drawn')
})

test('settings come from the address, with sensible fallbacks', () => {
  const read = (query) => settings(new URLSearchParams(query))

  assert.deepEqual(read(''), { seed: 1, bots: DEFAULT_BOTS })
  assert.deepEqual(read('seed=7&bots=0'), { seed: 7, bots: 0 })
  assert.deepEqual(read('seed=abc&bots=-2'), { seed: 1, bots: DEFAULT_BOTS })
  assert.deepEqual(read('bots=1.5'), { seed: 1, bots: DEFAULT_BOTS })
  assert.deepEqual(read('bots=9999'), { seed: 1, bots: MOST_BOTS })
})
