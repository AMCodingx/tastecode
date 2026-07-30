import { useEffect, useRef } from 'react'

const BAYER4 = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
].map((row) => row.map((value) => (value + 0.5) / 16))

const LIGHT = [237, 237, 237] as const

function paintDither(
  canvas: HTMLCanvasElement,
  bloomCanvas: HTMLCanvasElement,
  width: number,
  height: number,
  cell: number,
) {
  const context = canvas.getContext('2d')
  const bloomContext = bloomCanvas.getContext('2d')
  if (!context || !bloomContext || width <= 0 || height <= 0) return

  const canvasWidth = Math.ceil(width)
  const canvasHeight = Math.ceil(height)
  const columns = Math.max(4, Math.ceil(canvasWidth / cell))
  const rows = Math.max(4, Math.ceil(canvasHeight / cell))
  canvas.width = canvasWidth
  canvas.height = canvasHeight
  bloomCanvas.width = canvasWidth
  bloomCanvas.height = canvasHeight
  context.clearRect(0, 0, canvasWidth, canvasHeight)

  const capColumns = Math.max(4, Math.round(rows * 0.95))

  for (let y = 0; y < rows; y += 1) {
    for (let x = 0; x < columns; x += 1) {
      const threshold = BAYER4[y & 3]![x & 3]!
      const edgeProgress = columns <= 1 ? 1 : x / (columns - 1)
      const capProgress = Math.min(
        1,
        Math.max(0, (x - (columns - capColumns)) / Math.max(1, capColumns - 1)),
      )
      const capEase = capProgress * capProgress * (3 - 2 * capProgress)
      const trailDensity = 0.08 + edgeProgress ** 1.5 * 0.14
      const density = Math.max(trailDensity, capEase * 0.5)
      if (threshold > density) continue
      const trailAlpha = 0.07 + edgeProgress * 0.05
      const alpha = Math.min(1, trailAlpha + capEase * 0.88)
      context.fillStyle = `rgba(${LIGHT[0]},${LIGHT[1]},${LIGHT[2]},${alpha})`
      context.fillRect(x * cell, y * cell, cell, cell)
    }
  }

  bloomContext.clearRect(0, 0, canvasWidth, canvasHeight)
  bloomContext.drawImage(canvas, 0, 0)
}

export function DitherSlider({ active, cell = 4 }: { active: boolean; cell?: number }) {
  const wrapperRef = useRef<HTMLDivElement>(null)
  const ditherRef = useRef<HTMLCanvasElement>(null)
  const bloomRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const wrapper = wrapperRef.current
    const dither = ditherRef.current
    const bloom = bloomRef.current
    if (!wrapper || !dither || !bloom) return

    const paint = () => {
      paintDither(dither, bloom, wrapper.clientWidth, wrapper.clientHeight, cell)
    }

    paint()
    if (typeof ResizeObserver === 'undefined') return

    const observer = new ResizeObserver(paint)
    observer.observe(wrapper)
    return () => observer.disconnect()
  }, [cell])

  return (
    <div ref={wrapperRef} aria-hidden className={`dither-slider${active ? ' is-active' : ''}`}>
      <canvas ref={ditherRef} className="dither-slider__canvas dither-slider__texture" />
      <canvas ref={bloomRef} className="dither-slider__canvas dither-slider__bloom" />
    </div>
  )
}
