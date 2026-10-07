import { useEffect, useRef, useState } from 'react'
import { Volume2, VolumeX } from 'lucide-react'
import showreel from '../assets/traduko-sound-2.mp4'
import poster from '../assets/traduko-sound-2-poster.jpg'

export default function HeroVideo() {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [isMuted, setIsMuted] = useState(true)

  const toggleMute = () => {
    const video = videoRef.current
    if (!video) return
    const muted = !video.muted
    video.muted = muted
    setIsMuted(muted)
    if (!muted && video.paused) {
      void video.play().catch(() => { /* Keep the poster visible if playback is unavailable. */ })
    }
  }

  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)')
    const updatePlayback = () => {
      video.autoplay = !reducedMotion.matches
      if (reducedMotion.matches) video.pause()
      else void video.play().catch(() => { /* Keep the poster visible when autoplay is unavailable. */ })
    }

    updatePlayback()
    reducedMotion.addEventListener('change', updatePlayback)
    return () => reducedMotion.removeEventListener('change', updatePlayback)
  }, [])

  return (
    <figure className="hero-video hero-enter relative">
      <video
        ref={videoRef}
        src={showreel}
        poster={poster}
        width="1920"
        height="1080"
        muted={isMuted}
        onVolumeChange={(event) => setIsMuted(event.currentTarget.muted)}
        loop
        playsInline
        preload="metadata"
        aria-label="Traduko showreel: a translator that lives on your desktop"
      >
        Watch the <a href={showreel}>Traduko showreel</a>.
      </video>
      <button
        type="button"
        className="absolute bottom-4 left-4 z-10 flex size-11 items-center justify-center rounded-full bg-black/35 text-white shadow-sm backdrop-blur-xl transition-[background-color,transform] duration-150 hover:bg-black/50 active:scale-[0.96] focus-visible:outline-white"
        aria-label={isMuted ? 'Unmute video' : 'Mute video'}
        title={isMuted ? 'Unmute video' : 'Mute video'}
        onClick={toggleMute}
      >
        {isMuted ? <VolumeX size={20} strokeWidth={1.5} aria-hidden="true" /> : <Volume2 size={20} strokeWidth={1.5} aria-hidden="true" />}
      </button>
    </figure>
  )
}
