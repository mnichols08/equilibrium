// The page: joins a game to the Zubra components with app.js.
//
// Without `?server=` the game is in the page: Rust compiled to
// WebAssembly, one player and some bots. With it, the game is on a
// server and this page is one player's window on a shared world.

import './components/eq-components.js'
import { pageView, settings, startGame, startOnline } from './app.js'

const game = document.querySelector('#game')
const notice = document.querySelector('#notice')
const join = document.querySelector('#join')
const { seed, bots, server } = settings(new URLSearchParams(location.search))

/** Shows the game and sends what is typed in the box. */
function begin(view, send) {
  view.onCommand(send)
  const form = document.querySelector('#typed')
  form.addEventListener('submit', (event) => {
    event.preventDefault()
    send(form.elements.line.value)
    form.reset()
  })
}

function fail(message) {
  notice.textContent = message
  notice.classList.add('error')
  notice.hidden = false
}

/** Reads or writes the name last played under, where the browser lets us. */
function rememberedName(name) {
  try {
    if (name) localStorage.setItem('equilibrium.name', name)
    return localStorage.getItem('equilibrium.name') ?? ''
  } catch {
    return ''
  }
}

async function playAlone() {
  try {
    const { default: init, Shore } = await import('./pkg/eq_web.js')
    await init()
    const view = pageView(game)
    begin(view, startGame(new Shore(seed, bots), view))
    notice.hidden = true
    game.hidden = false
  } catch (error) {
    fail(`The game could not start: ${error.message}`)
  }
}

function playOnline() {
  notice.textContent = `A shared shoreline at ${server}.`
  join.elements.name.value = rememberedName()
  join.hidden = false
  join.addEventListener('submit', (event) => {
    event.preventDefault()
    const login = { name: join.elements.name.value.trim(), key: join.elements.key.value }
    join.hidden = true
    notice.classList.remove('error')
    notice.textContent = 'Walking out of the tide…'

    const view = pageView(game, { online: true })
    const shown = {
      ...view,
      // The first scene is the sign that the server let us in.
      show(scene, lines) {
        if (game.hidden) rememberedName(login.name)
        notice.hidden = true
        game.hidden = false
        view.show(scene, lines)
      },
    }
    let socket
    try {
      socket = new WebSocket(server)
    } catch (error) {
      join.hidden = false
      return fail(`Could not reach the server: ${error.message}`)
    }
    const send = startOnline(socket, shown, login, {
      onRefused(reason) {
        game.hidden = true
        join.hidden = false
        fail(reason)
      },
      onClosed() {
        if (game.hidden && join.hidden) {
          join.hidden = false
          fail('Could not reach the server.')
        }
      },
    })
    if (!playOnline.begun) begin(shown, (line) => playOnline.send(line))
    playOnline.begun = true
    playOnline.send = send
  })
}

if (server) playOnline()
else await playAlone()
