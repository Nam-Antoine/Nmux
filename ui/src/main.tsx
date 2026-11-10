import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './components/App'
import { installGlobalKeys } from './keys'
import { start } from './store'
import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
import './styles.css'
import './panel.css'

installGlobalKeys()
void start()

const fonts = Promise.all([document.fonts.load('14px "Geist Mono Variable"'), document.fonts.load('13px "Geist Variable"')])
const giveUp = new Promise((resolve) => setTimeout(resolve, 1500))

void Promise.race([fonts, giveUp]).then(() => {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>
  )
})
