import { useEffect, useState, useRef } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationPreview } from '@/fixtures/application-preview'
import { SettingsProvider } from '@/features/preferences/settings-store'
import { showcaseSource, showcaseDirectoryLoader } from './showcase-fixtures'
import { ShowcaseDesktop } from './showcase-desktop'
import { SiloMark } from '@/components/silo-mark'
import './app.css'
import './showcase.css'

const params = new URLSearchParams(location.search)
// `?variant=screens` renders only the three product screens on a transparent,
// tightly cropped frame (always the dark product UI) for use on other pages.
const screens = params.get('variant') === 'screens'
const initialDark = screens || params.get('theme') !== 'light'
const frameSize = screens ? { width: 1633, height: 1113 } : { width: 1600, height: 1100 }
document.documentElement.classList.toggle('dark', initialDark)
// Capture mode: no page chrome around the frame, and a transparent page behind it.
document.documentElement.classList.toggle('showcase-capture', screens || params.get('capture') === '1')

function ProductShot({ view, label, className }: { view: 'overview' | 'files' | 'github'; label: string; className: string }) {
  const shot = useRef<HTMLElement>(null)
  useEffect(() => {
    if (view !== 'overview') return
    const button = shot.current?.querySelector<HTMLButtonElement>('button[aria-label="SSH access controls for lab"]')
    if (button?.getAttribute('aria-expanded') === 'false') button.click()
  }, [view])
  return <section ref={shot} className={`showcase-shot ${className}`} aria-label={label}>
    <span className="showcase-shot-label">{label}</span>
    <div className="showcase-product">
      <SettingsProvider initialSettings={{ ...showcaseSource.preferences, alphaNoticeDismissed: true }}>
        <ApplicationPreview source={showcaseSource} initialRoute={view === 'github' ? { tab: 'github' } : { tab: 'computers', computerSection: view }} actions={{ listComputerDirectory: showcaseDirectoryLoader }} />
      </SettingsProvider>
    </div>
  </section>
}

export function Showcase() {
  const [dark, setDark] = useState(initialDark)
  const capture = params.get('capture') === '1' || screens
  const fit = () => capture ? 1 : Math.max(.15, Math.min(1, (window.innerWidth - 48) / frameSize.width, (window.innerHeight - 116) / frameSize.height))
  const [scale, setScale] = useState(fit)
  useEffect(() => { const resize = () => setScale(fit()); window.addEventListener('resize', resize); return () => window.removeEventListener('resize', resize) }, [capture])
  return <div className="showcase-workbench">
    <div className="showcase-canvas" style={{ width: frameSize.width * scale, height: frameSize.height * scale }}><div className={`showcase-frame${screens ? ' showcase-screens' : ''}`} style={{ transform: `scale(${scale})`, ...(screens ? frameSize : {}) }}>
      {!screens && <div className="showcase-atmosphere" aria-hidden="true" />}
      {!screens && <header className="showcase-heading">
        <div className="showcase-brand"><SiloMark aria-hidden="true" /><span>Silo</span></div>
        <span className="showcase-eyebrow">Available on macOS and Linux</span>
        <h1>Give your agents<br/>a computer<br/><span>of their own.</span></h1>
        <p>Linux computers running locally or remotely,<br/>with a desktop and computer use.</p>
      </header>}
      <ShowcaseDesktop />
      <ProductShot view="overview" label="Manage local and remote computers" className="showcase-overview" />
      <ProductShot view="github" label="Set fine-grained GitHub permissions per computer" className="showcase-github" />
    </div></div>
    {!capture && !screens && <footer className="showcase-tools" aria-label="Showcase controls">
      <span>GitHub hero · Draft 03</span>
      <button onClick={() => {document.documentElement.classList.toggle('dark', !dark); setDark(!dark)}}>{dark ? 'Light appearance' : 'Dark appearance'}</button>
      <span className="showcase-note">Production Silo UI · Illustrated Linux desktop · Illustrated agent task</span>
    </footer>}
  </div>
}
createRoot(document.getElementById('root')!).render(<Showcase />)
