import { useEffect, useRef, useState } from 'react'

type Point = readonly [number, number]

// The native idle bubble and pills from crates/blob, drawn with the same
// midpoint quadratic curves as crates/app/src/mascot_view.rs.
function nativeOutline(points: Point[]) {
  const midpoint = (a: Point, b: Point) => `${((a[0] + b[0]) / 2).toFixed(5)} ${((a[1] + b[1]) / 2).toFixed(5)}`
  return `M${midpoint(points[points.length - 1], points[0])}${points.map((point, index) => (
    `Q${point[0].toFixed(5)} ${point[1].toFixed(5)} ${midpoint(point, points[(index + 1) % points.length])}`
  )).join('')}Z`
}

function bubbleContains(x: number, y: number) {
  if (Math.abs(x) ** 2.9 + Math.abs((y + 0.1) / 0.8) ** 2.9 <= 1) return true
  for (let step = 0; step <= 24; step++) {
    const t = step / 24
    const dx = x - (-0.4 - 0.34 * t)
    const dy = y - (0.38 + 0.58 * t)
    const radius = 0.32 - 0.23 * t
    if (dx * dx + dy * dy <= radius * radius) return true
  }
  return false
}

function nativeBubble() {
  const count = 128
  const raw = Array.from({ length: count }, (_, index) => {
    const angle = index / count * Math.PI * 2
    const cos = Math.cos(angle)
    const sin = Math.sin(angle)
    let radius = 1.5
    while (radius > 0 && !bubbleContains(cos * radius, sin * radius)) radius -= 0.01
    let low = Math.max(0, radius)
    let high = Math.min(1.5, radius + 0.01)
    for (let step = 0; step < 10; step++) {
      const middle = (low + high) / 2
      if (bubbleContains(cos * middle, sin * middle)) low = middle
      else high = middle
    }
    return low
  })
  const kernel = [0.03, 0.11, 0.22, 0.28, 0.22, 0.11, 0.03]
  return nativeOutline(raw.map((_, index): Point => {
    const radius = kernel.reduce((sum, weight, offset) => sum + weight * raw[(index + count + offset - 3) % count], 0)
    const angle = index / count * Math.PI * 2
    return [radius * Math.cos(angle), radius * Math.sin(angle)]
  }))
}

function nativeEye(side: number) {
  const radius = 0.17 / 2
  const halfHeight = 0.46 / 2 - radius
  const tilt = 7 * Math.PI / 180
  const points: Point[] = []
  for (let corner = 0; corner < 4; corner++) {
    const cy = corner < 2 ? halfHeight : -halfHeight
    for (let step = 0; step <= 10; step++) {
      const angle = (corner + step / 10) * Math.PI / 2
      const x = radius * Math.cos(angle)
      const y = cy + radius * Math.sin(angle)
      points.push([
        0.02 + side * 0.46 + x * Math.cos(tilt) - y * Math.sin(tilt),
        -0.14 + x * Math.sin(tilt) + y * Math.cos(tilt),
      ])
    }
  }
  return nativeOutline(points)
}

const BODY_PATH = nativeBubble()
const EYE_PATHS = [nativeEye(-0.5), nativeEye(0.5)]

export default function HeroMascot() {
  const buttonRef = useRef<HTMLButtonElement>(null)
  const greetingRef = useRef<HTMLSpanElement>(null)
  const gazeRef = useRef<SVGGElement>(null)
  const reducedMotionRef = useRef(false)
  const greetingAnimationRef = useRef<Animation | null>(null)
  const greetingTimeoutRef = useRef<number | null>(null)
  const [greeting, setGreeting] = useState('')

  useEffect(() => {
    const button = buttonRef.current
    const eyes = gazeRef.current
    if (!button || !eyes) return

    const preference = window.matchMedia('(prefers-reduced-motion: reduce)')
    const bounds = button.getBoundingClientRect()
    let inView = bounds.bottom > 0 && bounds.top < window.innerHeight
    let frame = 0
    let previousTime = 0
    let pointer: Point | null = null
    let targetDirty = false
    const gaze = { x: 0, y: 0, targetX: 0, targetY: 0 }

    function centerEyes() {
      cancelAnimationFrame(frame)
      frame = 0
      previousTime = 0
      gaze.x = gaze.y = gaze.targetX = gaze.targetY = 0
      eyes!.setAttribute('transform', 'translate(0 0)')
    }

    function schedule() {
      if (!frame && !reducedMotionRef.current && inView && !document.hidden) frame = requestAnimationFrame(tick)
    }

    function tick(time: number) {
      frame = 0
      if (reducedMotionRef.current || !inView || document.hidden) return
      if (targetDirty && pointer) {
        const rect = button!.getBoundingClientRect()
        gaze.targetX = Math.max(-0.12, Math.min(0.12, (pointer[0] - rect.left - rect.width / 2) / 220 * 0.12))
        gaze.targetY = Math.max(-0.085, Math.min(0.085, (pointer[1] - rect.top - rect.height / 2) / 180 * 0.085))
        targetDirty = false
      }
      const elapsed = previousTime ? Math.min(48, time - previousTime) : 16
      previousTime = time
      const blend = 1 - Math.exp(-elapsed / 65)
      gaze.x += (gaze.targetX - gaze.x) * blend
      gaze.y += (gaze.targetY - gaze.y) * blend
      eyes!.setAttribute('transform', `translate(${gaze.x.toFixed(5)} ${gaze.y.toFixed(5)})`)
      if (Math.abs(gaze.targetX - gaze.x) + Math.abs(gaze.targetY - gaze.y) > 0.00015) schedule()
      else previousTime = 0
    }

    function followPointer(event: PointerEvent) {
      if (!event.isPrimary || reducedMotionRef.current || !inView || document.hidden) return
      pointer = [event.clientX, event.clientY]
      targetDirty = true
      schedule()
    }

    function resetGaze() {
      pointer = null
      targetDirty = false
      gaze.targetX = gaze.targetY = 0
      schedule()
    }

    function releaseTouch(event: PointerEvent) {
      if (event.pointerType === 'touch') resetGaze()
    }

    function refreshTarget() {
      if (pointer) {
        targetDirty = true
        schedule()
      }
    }

    function updateMotion() {
      reducedMotionRef.current = preference.matches
      button!.dataset.motion = preference.matches ? 'reduced' : 'animated'
      if (preference.matches) {
        resetGaze()
        centerEyes()
        greetingAnimationRef.current?.cancel()
      }
    }

    function updateVisibility() {
      button!.dataset.visible = String(inView && !document.hidden)
      if (!inView || document.hidden) centerEyes()
      else refreshTarget()
    }

    const observer = new IntersectionObserver(([entry]) => {
      inView = entry.isIntersecting
      updateVisibility()
    })
    updateMotion()
    updateVisibility()
    observer.observe(button)
    preference.addEventListener('change', updateMotion)
    window.addEventListener('pointermove', followPointer, { passive: true })
    window.addEventListener('pointerdown', followPointer, { passive: true })
    window.addEventListener('pointerup', releaseTouch, { passive: true })
    window.addEventListener('pointercancel', resetGaze, { passive: true })
    window.addEventListener('blur', resetGaze)
    window.addEventListener('resize', refreshTarget, { passive: true })
    window.addEventListener('scroll', refreshTarget, { passive: true })
    document.addEventListener('pointerleave', resetGaze)
    document.addEventListener('visibilitychange', updateVisibility)

    return () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
      preference.removeEventListener('change', updateMotion)
      window.removeEventListener('pointermove', followPointer)
      window.removeEventListener('pointerdown', followPointer)
      window.removeEventListener('pointerup', releaseTouch)
      window.removeEventListener('pointercancel', resetGaze)
      window.removeEventListener('blur', resetGaze)
      window.removeEventListener('resize', refreshTarget)
      window.removeEventListener('scroll', refreshTarget)
      document.removeEventListener('pointerleave', resetGaze)
      document.removeEventListener('visibilitychange', updateVisibility)
      greetingAnimationRef.current?.cancel()
      if (greetingTimeoutRef.current !== null) window.clearTimeout(greetingTimeoutRef.current)
    }
  }, [])

  function sayHello() {
    greetingAnimationRef.current?.cancel()
    const sprite = greetingRef.current
    if (sprite && !reducedMotionRef.current && typeof sprite.animate === 'function') {
      greetingAnimationRef.current = sprite.animate([
        { transform: 'translateY(0) rotate(0) scale(1)', offset: 0 },
        { transform: 'translateY(-5px) rotate(-5deg) scale(1.035)', offset: 0.35 },
        { transform: 'translateY(-2px) rotate(3deg) scale(1.01)', offset: 0.7 },
        { transform: 'translateY(0) rotate(0) scale(1)', offset: 1 },
      ], { duration: 650, easing: 'cubic-bezier(0.2, 0.7, 0.2, 1)' })
    }
    setGreeting('Bonjour! Hello!')
    if (greetingTimeoutRef.current !== null) window.clearTimeout(greetingTimeoutRef.current)
    greetingTimeoutRef.current = window.setTimeout(() => setGreeting(''), 1600)
  }

  return (
    <div className="hero-mascot-wrap mx-auto mb-6 w-[112px]">
      <button ref={buttonRef} type="button" className="hero-mascot block h-[112px] w-[112px] rounded-full bg-transparent p-0" aria-label="Say hello to Traduko" data-motion="animated" data-visible="true" onClick={sayHello}>
        <span className="hero-mascot-idle block h-full w-full">
          <span ref={greetingRef} className="hero-mascot-greeting block h-full w-full origin-[50%_84%]">
            <svg viewBox="-1.08 -0.98 2.16 2.12" className="block h-full w-full overflow-visible" aria-hidden="true" focusable="false">
              <path d={BODY_PATH} fill="#F45A1C" />
              <g ref={gazeRef} className="hero-mascot-gaze">
                <g className="hero-mascot-blink" fill="#fff">
                  {EYE_PATHS.map((path, index) => <path key={index} d={path} />)}
                </g>
              </g>
            </svg>
          </span>
        </span>
      </button>
      <span className="sr-only" role="status" aria-live="polite" aria-atomic="true">{greeting}</span>
    </div>
  )
}
