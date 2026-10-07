import { useEffect, useState } from 'react'
import { Check } from 'lucide-react'
import DownloadButton from './components/DownloadButton'
import Features from './components/Features'
import HeroShader from './components/HeroShader'
import HeroVideo from './components/HeroVideo'
import HeroMascot from './components/HeroMascot'

function useScrollReveals() {
  useEffect(() => {
    const elements = Array.from(document.querySelectorAll<HTMLElement>('.reveal'))
    const observer = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add('is-visible')
          observer.unobserve(entry.target)
        }
      })
    }, { threshold: 0.08 })
    elements.forEach((element) => { element.classList.add('will-reveal'); observer.observe(element) })
    return () => { observer.disconnect(); elements.forEach((element) => element.classList.remove('will-reveal')) }
  }, [])
}

function App() {
  const [isScrolled, setIsScrolled] = useState(() => typeof window !== 'undefined' && window.scrollY > 8)
  useScrollReveals()

  useEffect(() => {
    const updateScrollState = () => setIsScrolled(window.scrollY > 8)
    window.addEventListener('scroll', updateScrollState, { passive: true })
    return () => window.removeEventListener('scroll', updateScrollState)
  }, [])

  return (
    <>
      <a href="#main" className="skip-link">Skip to content</a>
      <header className={`site-header${isScrolled ? ' is-scrolled' : ''}`}>
        <nav className="nav-shell" aria-label="Main navigation">
          <a href="#" className="brand" aria-label="Traduko home">
            <img src="/traduko-icon.png" width="42" height="42" alt="" />
            <span>Traduko<span className="brand-dot">.</span></span>
          </a>
          <DownloadButton compact />
        </nav>
      </header>

      <main id="main">
        <section className="hero section-shell" aria-labelledby="hero-heading">
          <HeroShader />
          <div className="hero-intro">
            <HeroMascot />
            <h1 id="hero-heading" className="hero-heading hero-enter">
              Translate anytime<br />
              with a little companion.
            </h1>
            <p className="hero-description hero-enter">Meet Traduko. Your friendly desktop translator.<br className="hidden sm:block" /> Always a click away. Entirely on your Mac.</p>
            <div className="hero-actions hero-enter"><DownloadButton /></div>
            <p className="hero-footnote hero-enter">Made for macOS 13 and later <span>·</span> No account needed</p>
          </div>

          <HeroVideo />
        </section>

        <Features />

        <section className="closing-section section-shell" aria-labelledby="closing-heading">
          <div className="closing-card reveal">
            <HeroShader placement="card" />
            <img src="/traduko-icon.png" width="104" height="104" alt="" className="closing-icon" loading="lazy" />
            <h2 id="closing-heading" className="section-heading mt-8">Make room for<br />a little understanding.</h2>
            <p className="mx-auto mt-5 max-w-[420px] text-[17px] leading-relaxed text-[#756b60]">One click. A few words. A little closer.<br />Your Mac has a new friend.</p>
            <div className="mt-8"><DownloadButton /></div>
            <div className="mt-5 flex items-center justify-center gap-2 text-[12px] text-[#756b60]"><Check size={13} strokeWidth={1.5} /><span>macOS 13 and later</span></div>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <HeroShader placement="footer" />
        <div className="footer-content section-shell">
          <a href="#" className="brand"><img src="/traduko-icon.png" width="32" height="32" alt="" /><span>Traduko<span className="brand-dot">.</span></span></a>
          <p>A little translation. A lot of heart.</p>
          <span>Made for Mac, with care.</span>
        </div>
      </footer>
    </>
  )
}

export default App
