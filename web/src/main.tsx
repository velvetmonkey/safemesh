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
  const showWorkerNotice = (message: string, reload = false) => {
    const notice = document.createElement('aside')
    notice.setAttribute('role', 'status')
    notice.style.cssText = 'position:fixed;bottom:1rem;left:1rem;right:1rem;z-index:1000;padding:1rem;background:#fff;color:#111;border:2px solid #111;box-shadow:0 2px 12px #0003'
    notice.textContent = message
    if (reload) {
      const button = document.createElement('button')
      button.textContent = 'Reload now'
      button.style.marginLeft = '1rem'
      button.addEventListener('click', () => window.location.reload())
      notice.append(button)
    }
    document.body.append(notice)
  }

  let hadController = Boolean(navigator.serviceWorker.controller)
  navigator.serviceWorker.addEventListener('controllerchange', () => {
    if (hadController) showWorkerNotice('New SafeMesh content is active. Reload to see the latest version.', true)
    hadController = true
  })

  const register = () => {
    navigator.serviceWorker.register(`${import.meta.env.BASE_URL}sw.js`, { updateViaCache: 'none' }).catch((error) => {
      console.error('SafeMesh service worker registration failed.', error)
      showWorkerNotice('SafeMesh could not enable offline access. Reload to try again.')
    })
  }
  // WASM initialization can finish after the window's load event.
  if (document.readyState === 'complete') register()
  else window.addEventListener('load', register, { once: true })
}
