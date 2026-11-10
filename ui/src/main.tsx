import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './components/App'
import { installGlobalKeys } from './keys'
import { start } from './store'
import './styles.css'

installGlobalKeys()
void start()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
