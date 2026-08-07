import { describe, expect, it } from 'vitest'
import type { Model } from '@harness/contracts'
import {
  agentMark,
  choicesFor,
  filterModelChoicesByQuery,
  resolveReasoningEffort,
} from './model-catalog.js'

const model: Model = {
  id: 'shared-model',
  displayName: 'Shared model',
  isDefault: true,
  reasoningEfforts: [],
  serviceTiers: [],
}

function reasoningModel(reasoningEfforts: string[], defaultReasoningEffort?: string): Model {
  return {
    ...model,
    reasoningEfforts,
    ...(defaultReasoningEffort ? { defaultReasoningEffort } : {}),
  }
}

describe('model catalog', () => {
  it('keeps matching model ids separate across connections', () => {
    const first = choicesFor(
      { provider: 'api', connectionId: 'work', sourceName: 'Work', mark: 'openai' },
      [model],
    )[0]
    const second = choicesFor(
      { provider: 'api', connectionId: 'personal', sourceName: 'Personal', mark: 'openai' },
      [model],
    )[0]

    expect(first?.key).not.toBe(second?.key)
  })

  it.each([
    ['gemini', 'gemini'],
    ['kimi', 'kimi'],
    ['qwen', 'qwen'],
    ['another-agent', 'acp'],
  ] as const)('uses the correct mark for %s', (agent, mark) => {
    expect(agentMark(agent)).toBe(mark)
  })

  it('can omit a fake fallback when discovery is authoritative', () => {
    expect(
      choicesFor(
        {
          provider: 'acp',
          sourceName: 'Kimi CLI',
          mark: 'kimi',
          agent: { id: 'kimi', name: 'Kimi CLI' },
        },
        [],
        false,
      ),
    ).toEqual([])
  })

  it('filters model choices by case-insensitive words from their visible name or id', () => {
    const choices = choicesFor({ provider: 'opencode', sourceName: 'OpenCode', mark: 'opencode' }, [
      { ...model, id: 'openrouter/claude-opus-5', displayName: 'OpenRouter · Claude Opus 5' },
      { ...model, id: 'qwen/qwen3.8-max', displayName: 'OpenCode Go · Qwen3.8 Max' },
    ])

    expect(filterModelChoicesByQuery(choices, 'OPUS openrouter')).toEqual([choices[0]])
    expect(filterModelChoicesByQuery(choices, 'qwen3.8-max')).toEqual([choices[1]])
    expect(filterModelChoicesByQuery(choices, '  ')).toBe(choices)
  })

  it('keeps an effort that the next model supports', () => {
    expect(
      resolveReasoningEffort({
        currentEffort: 'medium',
        currentModel: reasoningModel(['low', 'medium', 'high']),
        nextModel: reasoningModel(['low', 'medium', 'high', 'max'], 'low'),
      }),
    ).toBe('medium')
  })

  it('keeps highest effort at the highest stop when the next model adds higher labels', () => {
    expect(
      resolveReasoningEffort({
        currentEffort: 'high',
        currentModel: reasoningModel(['low', 'medium', 'high']),
        nextModel: reasoningModel(['low', 'medium', 'high', 'max', 'ultra'], 'low'),
      }),
    ).toBe('ultra')
  })

  it.each(['max', 'ultra'])("maps %s to the next model's highest supported effort", (effort) => {
    expect(
      resolveReasoningEffort({
        currentEffort: effort,
        currentModel: reasoningModel(['low', 'medium', 'high', 'max', 'ultra']),
        nextModel: reasoningModel(['low', 'medium', 'high'], 'low'),
      }),
    ).toBe('high')
  })
})
