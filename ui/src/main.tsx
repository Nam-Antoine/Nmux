import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { beat, note } from './api'
import { App } from './components/App'
import { Guard } from './components/Guard'
import { installGlobalKeys } from './keys'
import { start } from './store'
import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
import './styles.css'
import './panel.css'

window.addEventListener('error', (e) => note(`error: ${e.message} at ${e.filename}:${e.lineno}`))
window.addEventListener('unhandledrejection', (e) => note(`unhandled: ${e.reason instanceof Error ? (e.reason.stack ?? e.reason.message) : String(e.reason)}`))
beat()
window.setInterval(beat, 5000)

installGlobalKeys()
void start()

const fonts = Promise.all([document.fonts.load('14px "Geist Mono Variable"'), document.fonts.load('13px "Geist Variable"')])
const giveUp = new Promise((resolve) => setTimeout(resolve, 1500))

void Promise.race([fonts, giveUp]).then(() => {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <Guard>
        <App />
      </Guard>
    </StrictMode>
  )
})
