// The page: loads the game (Rust compiled to WebAssembly) and the Zubra
// components, then joins them with app.js.

import init, { Shore } from './pkg/eq_web.js'
import './components/eq-components.js'
import { pageView, settings, startGame } from './app.js'

const game = document.querySelector('#game')
const notice = document.querySelector('#notice')

try {
  await init()
  const { seed, bots } = settings(new URLSearchParams(location.search))
  const view = pageView(game)
  const send = startGame(new Shore(seed, bots), view)
  view.onCommand(send)

  const form = document.querySelector('#typed')
  form.addEventListener('submit', (event) => {
    event.preventDefault()
    send(form.elements.line.value)
    form.reset()
  })

  notice.hidden = true
  game.hidden = false
} catch (error) {
  notice.textContent = `The game could not start: ${error.message}`
  notice.classList.add('error')
}
