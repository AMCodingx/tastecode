import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Download, Minus, Plus, X } from 'lucide-react'

const MIN_ZOOM = 0.5
const MAX_ZOOM = 3
const ZOOM_STEP = 0.25

export function ImageViewer(props: { src: string; name: string; onClose: () => void }) {
  const [zoom, setZoom] = useState(1)
  const viewport = useRef<HTMLDivElement>(null)
  const previousZoom = useRef(zoom)
  const close = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const previouslyFocused = document.activeElement as HTMLElement | null
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    close.current?.focus()

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault()
      props.onClose()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      document.body.style.overflow = previousOverflow
      previouslyFocused?.focus()
    }
  }, [props.onClose])

  useEffect(() => {
    const element = viewport.current
    const oldZoom = previousZoom.current
    previousZoom.current = zoom
    if (!element || oldZoom === zoom) return

    const factor = zoom / oldZoom
    element.scrollLeft =
      (element.scrollLeft + element.clientWidth / 2) * factor - element.clientWidth / 2
    element.scrollTop =
      (element.scrollTop + element.clientHeight / 2) * factor - element.clientHeight / 2
  }, [zoom])

  return createPortal(
    <div
      className="image-viewer"
      role="dialog"
      aria-modal="true"
      aria-label={`Preview ${props.name}`}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) props.onClose()
      }}
    >
      <div className="image-viewer__actions">
        <a
          className="image-viewer__action"
          href={props.src}
          download={props.name}
          aria-label="Download image"
          title="Download"
        >
          <Download size={18} aria-hidden />
        </a>
        <button
          ref={close}
          className="image-viewer__action"
          type="button"
          onClick={props.onClose}
          aria-label="Close image viewer"
          title="Close"
        >
          <X size={19} aria-hidden />
        </button>
      </div>

      <div
        ref={viewport}
        className="image-viewer__viewport"
        onMouseDown={(event) => {
          if (zoom <= 1 && event.target === event.currentTarget) props.onClose()
        }}
      >
        <div
          className="image-viewer__frame"
          style={{ width: `${zoom * 100}%`, height: `${zoom * 100}%` }}
          onMouseDown={(event) => {
            if (zoom <= 1 && event.target === event.currentTarget) props.onClose()
          }}
        >
          <img src={props.src} alt={props.name} draggable={false} />
        </div>
      </div>

      <div className="image-viewer__zoom" aria-label="Image zoom controls">
        <button
          type="button"
          onClick={() => setZoom((current) => Math.max(MIN_ZOOM, current - ZOOM_STEP))}
          disabled={zoom === MIN_ZOOM}
          aria-label="Zoom out"
        >
          <Minus size={16} aria-hidden />
        </button>
        <output aria-live="polite">{Math.round(zoom * 100)}%</output>
        <button
          type="button"
          onClick={() => setZoom((current) => Math.min(MAX_ZOOM, current + ZOOM_STEP))}
          disabled={zoom === MAX_ZOOM}
          aria-label="Zoom in"
        >
          <Plus size={16} aria-hidden />
        </button>
      </div>
    </div>,
    document.body,
  )
}
