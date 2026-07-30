import {
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from 'react'
import type { Model } from '@harness/contracts'
import { ChevronDown, ChevronRight, Zap } from 'lucide-react'
import { Menu } from './Menu.js'

const SLIDER_THUMB_SIZE = 28
const TRIGGER_LABEL = 'Model and reasoning'
const DIALOG_LABEL = 'Model and reasoning'
const SLIDER_LABEL = 'Reasoning effort'

type ModelSelectorProps = {
  models: Model[]
  modelId: string | undefined
  effort: string | undefined
  serviceTier: string | undefined
  disabled: boolean
  onModelChange: (id: string) => void
  onEffortChange: (value: string) => void
  onServiceTierChange: (value: string | undefined) => void
}

export function getCompactModelName(displayName: string | undefined): string {
  if (!displayName) return 'Model'
  return displayName
    .replace(/^gpt[-\s]*/i, '')
    .replace(/-/g, ' ')
    .trim()
}

export function getFriendlyEffortLabel(value: string | undefined): string {
  if (!value) return 'Automatic'
  if (value.toLowerCase() === 'xhigh') return 'Extra High'
  if (value.toLowerCase() === 'xlow') return 'Extra Low'
  return value
    .replace(/[_-]+/g, ' ')
    .replace(/\bx([a-z])/gi, (_, letter: string) => `extra ${letter}`)
    .replace(/\b\w/g, (letter) => letter.toUpperCase())
}

export function getEffortIndexFromPointer(input: {
  clientX: number
  left: number
  width: number
  stopCount: number
}): number {
  if (input.stopCount <= 1 || input.width <= SLIDER_THUMB_SIZE) {
    return 0
  }
  const travelWidth = input.width - SLIDER_THUMB_SIZE
  const relativeX = input.clientX - input.left - SLIDER_THUMB_SIZE / 2
  const progress = Math.min(1, Math.max(0, relativeX / travelWidth))
  return Math.round(progress * (input.stopCount - 1))
}

export function getFastServiceTier(
  model: Model | undefined,
): { id: string; name: string; description: string } | undefined {
  return model?.serviceTiers.find((tier) => {
    const id = tier.id.trim().toLowerCase()
    const name = tier.name.trim().toLowerCase()
    return id === 'priority' || id === 'fast' || name === 'fast'
  })
}

export function getFastModeOffValue(model: Model | undefined): string | undefined {
  const defaultTier = model?.defaultServiceTier ?? undefined
  if (!defaultTier) return undefined
  return defaultTier === getFastServiceTier(model)?.id ? undefined : defaultTier
}

function getSelectedModel(models: Model[], modelId: string | undefined): Model | undefined {
  return (
    models.find((entry) => entry.id === modelId) ??
    models.find((entry) => entry.isDefault) ??
    models[0]
  )
}

function getSelectedEffort(
  model: Model | undefined,
  effort: string | undefined,
): string | undefined {
  if (!model) return undefined
  if (effort && model.reasoningEfforts.includes(effort)) return effort
  return model.defaultReasoningEffort ?? model.reasoningEfforts[0]
}

function isFastModeEnabled(model: Model | undefined, serviceTier: string | undefined): boolean {
  return Boolean(serviceTier && getFastServiceTier(model)?.id === serviceTier)
}

function supportsServiceTier(model: Model | undefined, serviceTier: string | undefined): boolean {
  return Boolean(serviceTier && model?.serviceTiers.some((tier) => tier.id === serviceTier))
}

function getNextServiceTierForModel(input: {
  nextModel: Model
  currentModel: Model | undefined
  currentServiceTier: string | undefined
}): string | undefined {
  const { nextModel, currentModel, currentServiceTier } = input
  if (isFastModeEnabled(currentModel, currentServiceTier)) {
    return getFastServiceTier(nextModel)?.id ?? getFastModeOffValue(nextModel)
  }
  if (supportsServiceTier(nextModel, currentServiceTier)) {
    return currentServiceTier
  }
  return getFastModeOffValue(nextModel)
}

function setPointerCaptureSafe(target: HTMLDivElement, pointerId: number) {
  if (typeof target.setPointerCapture === 'function') {
    target.setPointerCapture(pointerId)
  }
}

function releasePointerCaptureSafe(target: HTMLDivElement, pointerId: number) {
  if (typeof target.releasePointerCapture === 'function') {
    target.releasePointerCapture(pointerId)
  }
}

function hasPointerCaptureSafe(target: HTMLDivElement, pointerId: number): boolean {
  return typeof target.hasPointerCapture === 'function' ? target.hasPointerCapture(pointerId) : true
}

function EffortGauge({ effort, options }: { effort: string | undefined; options: string[] }) {
  const selectedIndex = effort === undefined ? -1 : options.indexOf(effort)
  const progress =
    selectedIndex < 0 || options.length < 2 ? 0.5 : selectedIndex / (options.length - 1)
  const needleRotation = -60 + progress * 120

  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M3.34 19a10 10 0 1 1 17.32 0" />
      <path d="M12 14V8" transform={`rotate(${needleRotation} 12 14)`} />
    </svg>
  )
}

export function ModelSelector(props: ModelSelectorProps) {
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [pointerIndex, setPointerIndex] = useState<number | null>(null)
  const pointerIndexRef = useRef<number | null>(null)
  const modelsPanelId = useId()

  const model = getSelectedModel(props.models, props.modelId)
  const selectedEffort = getSelectedEffort(model, props.effort)
  const effortOptions = model?.reasoningEfforts ?? []
  const selectedIndex =
    selectedEffort === undefined ? -1 : Math.max(0, effortOptions.indexOf(selectedEffort))
  const displayIndex = pointerIndex ?? selectedIndex
  const displayedEffort = effortOptions[displayIndex] ?? selectedEffort
  const effortLabel = getFriendlyEffortLabel(displayedEffort)
  const fastTier = getFastServiceTier(model)
  const fastEnabled = isFastModeEnabled(model, props.serviceTier)
  const sliderDisabled = props.disabled || effortOptions.length <= 1
  const progress =
    displayIndex < 0 || effortOptions.length < 2 ? 0.5 : displayIndex / (effortOptions.length - 1)
  const thumbOffset = (0.5 - progress) * SLIDER_THUMB_SIZE
  const thumbLeft = `calc(${progress * 100}% + ${thumbOffset}px)`
  const sliderVars = {
    '--model-selector-slider-progress': String(progress),
    '--model-selector-slider-left': thumbLeft,
  } as CSSProperties

  const previewFromPointer = (event: PointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect()
    const nextIndex = getEffortIndexFromPointer({
      clientX: event.clientX,
      left: rect.left,
      width: rect.width,
      stopCount: effortOptions.length,
    })
    if (pointerIndexRef.current !== nextIndex) {
      pointerIndexRef.current = nextIndex
      setPointerIndex(nextIndex)
    }
  }

  const commitEffortIndex = (nextIndex: number) => {
    const nextValue = effortOptions[nextIndex]
    if (!nextValue || nextValue === selectedEffort) return
    props.onEffortChange(nextValue)
  }

  const handleSliderKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (sliderDisabled || effortOptions.length === 0) return
    let nextIndex = selectedIndex < 0 ? 0 : selectedIndex
    if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') {
      nextIndex = Math.max(0, nextIndex - 1)
    } else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') {
      nextIndex = Math.min(effortOptions.length - 1, nextIndex + 1)
    } else if (event.key === 'Home') {
      nextIndex = 0
    } else if (event.key === 'End') {
      nextIndex = effortOptions.length - 1
    } else {
      return
    }
    event.preventDefault()
    commitEffortIndex(nextIndex)
  }

  const handleModelSelect = (nextModel: Model) => {
    if (nextModel.id !== model?.id) {
      props.onModelChange(nextModel.id)
    }

    if (
      nextModel.reasoningEfforts.length > 0 &&
      (!selectedEffort || !nextModel.reasoningEfforts.includes(selectedEffort))
    ) {
      const nextEffort = nextModel.defaultReasoningEffort ?? nextModel.reasoningEfforts[0]
      if (nextEffort && nextEffort !== selectedEffort) {
        props.onEffortChange(nextEffort)
      }
    }

    const nextServiceTier = getNextServiceTierForModel({
      nextModel,
      currentModel: model,
      currentServiceTier: props.serviceTier,
    })
    if (nextServiceTier !== props.serviceTier) {
      props.onServiceTierChange(nextServiceTier)
    }
  }

  return (
    <Menu
      align="right"
      disabled={props.disabled}
      label={TRIGGER_LABEL}
      triggerClassName="menutrigger--model-selector"
      panelRole="dialog"
      panelLabel={DIALOG_LABEL}
      panelClassName="model-selector__menu"
      trigger={(open) => (
        <span className={`model-selector__trigger${open ? ' is-open' : ''}`}>
          <span className="model-selector__trigger-gauge">
            <EffortGauge effort={selectedEffort} options={effortOptions} />
          </span>
          <span className="model-selector__trigger-copy">
            <span className="model-selector__trigger-model">
              {getCompactModelName(model?.displayName)}
            </span>
            <span className="model-selector__trigger-effort">{effortLabel}</span>
          </span>
          <span className="model-selector__trigger-chevron" aria-hidden>
            <ChevronDown size={16} />
          </span>
        </span>
      )}
    >
      {(close) => (
        <div className="model-selector">
          <div className="model-selector__header">
            <button
              type="button"
              className={`model-selector__advanced${advancedOpen ? ' is-open' : ''}`}
              aria-controls={modelsPanelId}
              aria-expanded={advancedOpen}
              aria-label={advancedOpen ? 'Hide advanced model list' : 'Show advanced model list'}
              onClick={() => setAdvancedOpen((current) => !current)}
            >
              <span>Advanced</span>
              <span className="model-selector__advanced-chevron" aria-hidden>
                <ChevronRight size={16} />
              </span>
            </button>

            {fastTier ? (
              <button
                type="button"
                className={`model-selector__fast${fastEnabled ? ' is-on' : ''}`}
                aria-label={fastEnabled ? 'Disable fast mode' : 'Enable fast mode'}
                aria-pressed={fastEnabled}
                title={fastTier.description}
                onClick={() =>
                  props.onServiceTierChange(fastEnabled ? getFastModeOffValue(model) : fastTier.id)
                }
              >
                <span className="model-selector__fast-icon" aria-hidden>
                  <Zap size={16} />
                </span>
              </button>
            ) : null}
          </div>

          {effortOptions.length > 0 ? (
            <div className="model-selector__section">
              <div
                role="slider"
                tabIndex={sliderDisabled ? -1 : 0}
                aria-label={SLIDER_LABEL}
                aria-disabled={sliderDisabled}
                aria-valuemin={0}
                aria-valuemax={Math.max(0, effortOptions.length - 1)}
                aria-valuenow={Math.max(0, displayIndex)}
                aria-valuetext={effortLabel}
                className={`model-selector__slider${sliderDisabled ? ' is-disabled' : ''}${pointerIndex !== null ? ' is-dragging' : ''}`}
                style={sliderVars}
                onClick={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                }}
                onKeyDown={handleSliderKeyDown}
                onPointerDown={(event) => {
                  if (sliderDisabled) return
                  event.preventDefault()
                  event.stopPropagation()
                  setPointerCaptureSafe(event.currentTarget, event.pointerId)
                  previewFromPointer(event)
                }}
                onPointerMove={(event) => {
                  if (
                    !sliderDisabled &&
                    hasPointerCaptureSafe(event.currentTarget, event.pointerId)
                  ) {
                    previewFromPointer(event)
                  }
                }}
                onPointerUp={(event) => {
                  if (
                    sliderDisabled ||
                    !hasPointerCaptureSafe(event.currentTarget, event.pointerId)
                  ) {
                    return
                  }
                  event.preventDefault()
                  event.stopPropagation()
                  const nextIndex = pointerIndexRef.current ?? selectedIndex
                  releasePointerCaptureSafe(event.currentTarget, event.pointerId)
                  pointerIndexRef.current = null
                  setPointerIndex(null)
                  if (nextIndex >= 0) {
                    requestAnimationFrame(() => commitEffortIndex(nextIndex))
                  }
                }}
                onPointerCancel={(event) => {
                  if (hasPointerCaptureSafe(event.currentTarget, event.pointerId)) {
                    releasePointerCaptureSafe(event.currentTarget, event.pointerId)
                  }
                  pointerIndexRef.current = null
                  setPointerIndex(null)
                }}
              >
                <div className="model-selector__slider-track">
                  <div className="model-selector__slider-fill" />
                </div>

                <div className="model-selector__slider-stops" aria-hidden>
                  {effortOptions.map((option, index) => (
                    <span
                      key={option}
                      className={`model-selector__slider-stop${index <= displayIndex ? ' is-on' : ''}`}
                    />
                  ))}
                </div>

                <span className="model-selector__slider-thumb" aria-hidden />
              </div>
            </div>
          ) : null}

          <div
            id={modelsPanelId}
            className={`model-selector__models${advancedOpen ? ' is-open' : ''}`}
            hidden={!advancedOpen}
          >
            <div className="model-selector__section" role="group" aria-label="Models">
              {props.models.map((entry) => {
                const selected = entry.id === model?.id
                return (
                  <button
                    key={entry.id}
                    type="button"
                    className={`model-selector__model${selected ? ' is-selected' : ''}`}
                    aria-pressed={selected}
                    aria-label={`Use ${entry.displayName}`}
                    onClick={() => {
                      handleModelSelect(entry)
                      close()
                    }}
                  >
                    <span className="model-selector__model-copy">
                      <span className="model-selector__model-name">{entry.displayName}</span>
                      {entry.description ? (
                        <span className="model-selector__model-description">
                          {entry.description}
                        </span>
                      ) : null}
                    </span>
                  </button>
                )
              })}
            </div>
          </div>
        </div>
      )}
    </Menu>
  )
}
