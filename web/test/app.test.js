import assert from 'node:assert/strict'
import { test } from 'node:test'

import { DEFAULT_BOTS, MOST_BOTS, PROTOCOL_VERSION, settings, startGame, startOnline } from '../app.js'

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

  assert.deepEqual(read(''), { seed: 1, bots: DEFAULT_BOTS, server: null })
  assert.deepEqual(read('seed=7&bots=0'), { seed: 7, bots: 0, server: null })
  assert.deepEqual(read('seed=abc&bots=-2'), { seed: 1, bots: DEFAULT_BOTS, server: null })
  assert.deepEqual(read('bots=1.5'), { seed: 1, bots: DEFAULT_BOTS, server: null })
  assert.deepEqual(read('bots=9999'), { seed: 1, bots: MOST_BOTS, server: null })
})

/** A socket that records what is sent and lets the test play the server. */
function fakeSocket() {
  const listeners = {}
  return {
    sent: [],
    send(data) {
      this.sent.push(JSON.parse(data))
    },
    addEventListener: (type, listener) => (listeners[type] = listener),
    open: () => listeners.open({}),
    close: () => listeners.close({}),
    receive: (message) => listeners.message({ data: JSON.stringify(message) }),
  }
}

const LOGIN = { name: 'Ada', key: 'pass' }

test('a server address is read from the page address, and nonsense is not', () => {
  const read = (query) => settings(new URLSearchParams(query)).server
  assert.equal(read('server=ws://127.0.0.1:4000'), 'ws://127.0.0.1:4000')
  assert.equal(read('server=wss://play.example/world'), 'wss://play.example/world')
  assert.equal(read('server=http://example.com'), null)
  assert.equal(read('server=javascript:alert(1)'), null)
  assert.equal(read(''), null)
})

test('going online says hello, asking for scenes, once the socket opens', () => {
  const socket = fakeSocket()
  startOnline(socket, fakeView(), LOGIN)
  assert.deepEqual(socket.sent, [])
  socket.open()
  assert.deepEqual(socket.sent, [{ type: 'hello', version: PROTOCOL_VERSION, name: 'Ada', key: 'pass', scenes: true }])
})

test('nothing is drawn until the first scene, then every message redraws', () => {
  const socket = fakeSocket()
  const view = fakeView()
  startOnline(socket, view, LOGIN)
  socket.open()
  socket.receive({ type: 'welcome', version: 1, name: 'Ada', new: true })
  socket.receive({ type: 'lines', lines: ['Tidewatch Camp [haven]'] })
  assert.equal(view.frames.length, 0)

  socket.receive({ type: 'scene', scene: { clock: '0:00:01' } })
  assert.deepEqual(view.frames, [{ scene: { clock: '0:00:01' }, lines: ['Welcome, Ada.', 'Tidewatch Camp [haven]'] }])

  socket.receive({ type: 'lines', lines: ['Ben arrives from Ore Ledge.'] })
  assert.equal(view.frames.at(-1).lines.at(-1), 'Ben arrives from Ore Ledge.')
  assert.deepEqual(view.frames.at(-1).scene, { clock: '0:00:01' })
})

test('a returning player is welcomed back', () => {
  const socket = fakeSocket()
  const view = fakeView()
  startOnline(socket, view, LOGIN)
  socket.open()
  socket.receive({ type: 'welcome', version: 1, name: 'Ada', new: false })
  socket.receive({ type: 'scene', scene: {} })
  assert.deepEqual(view.frames.at(-1).lines, ['Welcome back, Ada.'])
})

test('typed lines go to the server and into the log; blank ones and early ones do not', () => {
  const socket = fakeSocket()
  const view = fakeView()
  const send = startOnline(socket, view, LOGIN)
  send('north')
  socket.open()
  socket.receive({ type: 'scene', scene: {} })
  send('   ')
  send(' north ')
  assert.deepEqual(socket.sent.slice(1), [{ type: 'line', text: 'north' }])
  assert.deepEqual(view.frames.at(-1).lines, ['> north'])
})

test('a refusal and a closed connection are reported', () => {
  const socket = fakeSocket()
  const view = fakeView()
  const heard = []
  const send = startOnline(socket, view, LOGIN, { onRefused: (reason) => heard.push(reason), onClosed: () => heard.push('closed') })
  socket.open()
  socket.receive({ type: 'refused', reason: 'That name is taken.' })
  socket.close()
  assert.deepEqual(heard, ['That name is taken.', 'closed'])
  send('look')
  assert.equal(socket.sent.length, 1, 'nothing is sent on a closed connection')
})
