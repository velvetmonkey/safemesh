import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

if ('serviceWorker' in navigator) {
  const register = () => {
    navigator.serviceWorker.register(`${import.meta.env.BASE_URL}sw.js`, { updateViaCache: 'none' }).catch((error) => {
      console.error('SafeMesh service worker registration failed.', error)
    })
  }
  // WASM initialization can finish after the window's load event.
  if (document.readyState === 'complete') register()
  else window.addEventListener('load', register, { once: true })
}
