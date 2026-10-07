import { useEffect, useRef } from 'react'

const FRAME_INTERVAL = 1000 / 30
const MAX_DPR = 1.5
const MAX_PARALLAX = 0.014

const shader = /* wgsl */ `
struct Uniforms {
  resolution: vec2<f32>,
  time: f32,
  padding: f32,
  pointer: vec2<f32>,
  padding2: vec2<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
  let positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>(3.0, -1.0),
    vec2<f32>(-1.0, 3.0)
  );
  return vec4<f32>(positions[index], 0.0, 1.0);
}

fn hash(p: vec2<f32>) -> f32 {
  var q = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
  q += vec3<f32>(dot(q, q.yzx + vec3<f32>(33.33)));
  return fract((q.x + q.y) * q.z);
}

fn noise(p: vec2<f32>) -> f32 {
  let cell = floor(p);
  let f = fract(p);
  let blend = f * f * (vec2<f32>(3.0) - 2.0 * f);
  return mix(
    mix(hash(cell), hash(cell + vec2<f32>(1.0, 0.0)), blend.x),
    mix(hash(cell + vec2<f32>(0.0, 1.0)), hash(cell + vec2<f32>(1.0)), blend.x),
    blend.y
  );
}

fn foliage(p: vec2<f32>) -> f32 {
  let grid = p * vec2<f32>(7.0, 9.0);
  let cell = floor(grid);
  let local = fract(grid);
  var leaves = 0.0;
  for (var y: i32 = -1; y <= 1; y++) {
    for (var x: i32 = -1; x <= 1; x++) {
      let neighbor = vec2<f32>(f32(x), f32(y));
      let seed = hash(cell + neighbor);
      let center = neighbor + vec2<f32>(seed, hash(cell + neighbor + vec2<f32>(17.0)));
      let angle = seed * 6.28318;
      let direction = vec2<f32>(cos(angle), sin(angle));
      let delta = local - center;
      let leaf = vec2<f32>(dot(delta, direction), dot(delta, vec2<f32>(-direction.y, direction.x)));
      let distance = length(leaf / vec2<f32>(0.64, 0.27));
      leaves += (1.0 - smoothstep(0.2, 1.55, distance)) * (0.35 + seed * 0.45);
    }
  }
  return clamp(leaves, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
  let uv = position.xy / uniforms.resolution;
  let aspect = min(uniforms.resolution.x / uniforms.resolution.y, 2.4);
  let p = (uv + uniforms.pointer) * vec2<f32>(aspect, 1.0);
  let t = uniforms.time;

  // Continuous sampling makes the shapes fall smoothly; the footer's flip makes them rise.
  // No wrapping or reset is needed, so new shapes enter without repeating seams.
  let flowingP = p - vec2<f32>(0.0, t * 0.075);
  let wind = vec2<f32>(
    sin(t * 0.48) * 0.082 + sin(t * 0.91) * 0.016,
    cos(t * 0.36) * 0.045
  );
  let dapple = noise(flowingP * 3.1 + wind) * 0.68 + noise(flowingP * 8.0 + wind * 0.5) * 0.32;
  let canopy = clamp(dapple * 0.62 + foliage(flowingP + wind) * 0.48, 0.0, 1.0);
  let shade = smoothstep(0.30, 0.82, canopy);
  let lightOpening = smoothstep(0.54, 0.75, noise(flowingP * 5.0 + vec2<f32>(7.0, 12.0) - wind));
  let sunDistance = length((uv - vec2<f32>(0.61, 0.14)) * vec2<f32>(1.2, 1.8));
  let sunlight = exp(-sunDistance * sunDistance / 0.09);

  // Traduko's #F45A1C pigment, softened into apricot, peach, and cream light.
  let logoOrange = vec3<f32>(244.0 / 255.0, 90.0 / 255.0, 28.0 / 255.0);
  let apricot = vec3<f32>(0.98792, 0.81882, 0.75075);
  let peachLight = vec3<f32>(0.99569, 0.93529, 0.91098);
  var color = mix(apricot, logoOrange, shade * 0.55);
  color = mix(color, peachLight, lightOpening * 0.70);
  color = mix(color, vec3<f32>(1.0, 0.97, 0.94), sunlight * 0.75);

  // Keep the central reading area airy and dissolve the canopy into white below.
  let sideLight = mix(0.35, 1.0, smoothstep(0.03, 0.42, abs(uv.x - 0.5)));
  let fade = 1.0 - smoothstep(0.04, 0.85, uv.y);
  let alpha = min(0.14 + shade * 0.48 + lightOpening * 0.12, 0.58) * sideLight * fade;

  // The premultiplied surface lets the page's white background show through.
  return vec4<f32>(color * alpha, alpha);
}
`

type HeroShaderProps = {
  placement?: 'hero' | 'footer' | 'card'
}

export default function HeroShader({ placement = 'hero' }: HeroShaderProps) {
  const wrapperRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const wrapper = wrapperRef.current
    const canvas = canvasRef.current
    if (!wrapper || !canvas) return

    const motionPreference = window.matchMedia('(prefers-reduced-motion: reduce)')
    let reducedMotion = motionPreference.matches
    let disposed = false
    let failed = false
    let ready = false
    let frame = 0
    let lastTick = 0
    let lastFrame = 0
    let time = 0
    let needsFrame = true
    let device: GPUDevice | null = null
    let context: GPUCanvasContext | null = null
    let uniformBuffer: GPUBuffer | null = null
    let renderFrame: () => void = () => {}
    const pointer = { x: 0, y: 0, targetX: 0, targetY: 0 }
    const bounds = wrapper.getBoundingClientRect()
    let inView = bounds.bottom > 0 && bounds.top < window.innerHeight

    wrapper.dataset.renderer = 'fallback'
    wrapper.dataset.error = 'initializing'
    wrapper.dataset.motion = reducedMotion ? 'reduced' : 'animated'

    function pause() {
      cancelAnimationFrame(frame)
      frame = 0
      lastTick = 0
      resetPointer()
    }

    function resetPointer() {
      pointer.targetX = 0
      pointer.targetY = 0
    }

    function onPointerMove(event: PointerEvent) {
      if (reducedMotion || !inView || document.hidden || event.pointerType === 'touch') return
      const rect = wrapper!.getBoundingClientRect()
      if (!rect.width || !rect.height || event.clientX < rect.left || event.clientX > rect.right
        || event.clientY < rect.top || event.clientY > rect.bottom) {
        resetPointer()
        return
      }
      pointer.targetX = ((event.clientX - rect.left) / rect.width - 0.5) * MAX_PARALLAX * 2
      pointer.targetY = ((event.clientY - rect.top) / rect.height - 0.5) * MAX_PARALLAX * 2
    }

    function releaseGpu() {
      renderFrame = () => {}
      device?.removeEventListener('uncapturederror', onGpuError)
      context?.unconfigure()
      uniformBuffer?.destroy()
      device?.destroy()
      context = null
      uniformBuffer = null
      device = null
    }

    function fallback(reason: string) {
      if (disposed || failed) return
      failed = true
      ready = false
      pause()
      wrapper!.dataset.renderer = 'fallback'
      wrapper!.dataset.error = reason
      releaseGpu()
    }

    function onGpuError(event: Event) {
      event.preventDefault()
      fallback('render')
    }

    function schedule() {
      if (!disposed && !failed && ready && inView && !document.hidden
        && !frame && (!reducedMotion || needsFrame)) {
        frame = requestAnimationFrame(tick)
      }
    }

    function tick(now: number) {
      frame = 0
      if (disposed || failed || !inView || document.hidden) return
      if (lastTick && !reducedMotion) time += Math.min((now - lastTick) / 1000, 0.1)
      lastTick = now
      if (needsFrame || now - lastFrame >= FRAME_INTERVAL) {
        try {
          renderFrame()
          wrapper!.dataset.renderer = 'webgpu'
          delete wrapper!.dataset.error
          needsFrame = false
          lastFrame = now
        } catch {
          fallback('render')
        }
      }
      schedule()
    }

    function resize() {
      const rect = wrapper!.getBoundingClientRect()
      const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR)
      const maxDimension = device?.limits.maxTextureDimension2D ?? 4096
      const width = Math.max(1, Math.round(rect.width * dpr))
      const height = Math.max(1, Math.round(rect.height * dpr))
      const scale = Math.min(1, maxDimension / width, maxDimension / height)
      const nextWidth = Math.max(1, Math.floor(width * scale))
      const nextHeight = Math.max(1, Math.floor(height * scale))
      if (canvas!.width !== nextWidth || canvas!.height !== nextHeight) {
        canvas!.width = nextWidth
        canvas!.height = nextHeight
        needsFrame = true
      }
      schedule()
    }

    function onVisibilityChange() {
      if (document.hidden) pause()
      else schedule()
    }

    function onMotionChange(event: MediaQueryListEvent) {
      reducedMotion = event.matches
      if (reducedMotion) {
        pointer.x = 0
        pointer.y = 0
      }
      wrapper!.dataset.motion = reducedMotion ? 'reduced' : 'animated'
      needsFrame = true
      pause()
      schedule()
    }

    const resizeObserver = new ResizeObserver(resize)
    const intersectionObserver = new IntersectionObserver(([entry]) => {
      inView = entry.isIntersecting
      if (inView) schedule()
      else pause()
    })
    resizeObserver.observe(wrapper)
    intersectionObserver.observe(wrapper)
    motionPreference.addEventListener('change', onMotionChange)
    document.addEventListener('visibilitychange', onVisibilityChange)
    window.addEventListener('resize', resize, { passive: true })
    window.addEventListener('pointermove', onPointerMove, { passive: true })
    window.addEventListener('blur', resetPointer)
    document.addEventListener('pointerleave', resetPointer)

    async function initialize() {
      let stage = 'adapter'
      try {
        if (!navigator.gpu) {
          fallback('unavailable')
          return
        }
        const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'low-power' })
        if (disposed) return
        if (!adapter) {
          fallback('adapter')
          return
        }

        stage = 'device'
        const gpuDevice = await adapter.requestDevice()
        if (disposed) {
          gpuDevice.destroy()
          return
        }
        device = gpuDevice
        void gpuDevice.lost.then(() => fallback('device-lost'))
        gpuDevice.addEventListener('uncapturederror', onGpuError)
        const gpuContext = canvas!.getContext('webgpu')
        if (!gpuContext) {
          fallback('unavailable')
          return
        }
        context = gpuContext

        stage = 'shader'
        const format = navigator.gpu.getPreferredCanvasFormat()
        gpuDevice.pushErrorScope('validation')
        const module = gpuDevice.createShaderModule({ label: 'Traduko filtered daylight', code: shader })
        const pipeline = await gpuDevice.createRenderPipelineAsync({
          label: 'Traduko hero background',
          layout: 'auto',
          vertex: { module, entryPoint: 'vertex' },
          fragment: { module, entryPoint: 'fragment', targets: [{ format }] },
          primitive: { topology: 'triangle-list' },
        })
        const validationError = await gpuDevice.popErrorScope()
        if (disposed || failed) return
        if (validationError) {
          fallback('shader')
          return
        }

        const buffer = gpuDevice.createBuffer({
          label: 'Hero resolution and time',
          size: 32,
          usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
        })
        uniformBuffer = buffer
        const bindGroup = gpuDevice.createBindGroup({
          layout: pipeline.getBindGroupLayout(0),
          entries: [{ binding: 0, resource: { buffer } }],
        })
        const uniforms = new Float32Array(8)
        gpuContext.configure({ device: gpuDevice, format, alphaMode: 'premultiplied' })

        renderFrame = () => {
          uniforms[0] = canvas!.width
          uniforms[1] = canvas!.height
          uniforms[2] = reducedMotion ? 0 : time
          if (!reducedMotion) {
            pointer.x += (pointer.targetX - pointer.x) * 0.08
            pointer.y += (pointer.targetY - pointer.y) * 0.08
          }
          uniforms[4] = reducedMotion ? 0 : pointer.x
          uniforms[5] = reducedMotion ? 0 : pointer.y
          gpuDevice.queue.writeBuffer(buffer, 0, uniforms)
          const encoder = gpuDevice.createCommandEncoder()
          const pass = encoder.beginRenderPass({
            colorAttachments: [{
              view: gpuContext.getCurrentTexture().createView(),
              clearValue: { r: 0, g: 0, b: 0, a: 0 },
              loadOp: 'clear',
              storeOp: 'store',
            }],
          })
          pass.setPipeline(pipeline)
          pass.setBindGroup(0, bindGroup)
          pass.draw(3)
          pass.end()
          gpuDevice.queue.submit([encoder.finish()])
        }
        ready = true
        resize()
        schedule()
      } catch {
        fallback(stage)
      }
    }

    void initialize()

    return () => {
      disposed = true
      pause()
      resizeObserver.disconnect()
      intersectionObserver.disconnect()
      motionPreference.removeEventListener('change', onMotionChange)
      document.removeEventListener('visibilitychange', onVisibilityChange)
      window.removeEventListener('resize', resize)
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('blur', resetPointer)
      document.removeEventListener('pointerleave', resetPointer)
      releaseGpu()
    }
  }, [])

  return (
    <div ref={wrapperRef} className={`hero-shader${placement === 'hero' ? '' : ` ${placement}-shader`}`} aria-hidden="true" data-renderer="fallback" data-motion="animated">
      <canvas ref={canvasRef} className="hero-shader-canvas" />
    </div>
  )
}
