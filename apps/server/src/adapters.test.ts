import { afterEach, describe, expect, it, vi } from 'vitest'

/**
 * Listing OpenCode models spawns a real `opencode serve` process for the
 * duration of the call. These tests pin the property that made a renderer
 * refresh loop harmless again: concurrent listings share one adapter run
 * instead of forking one process each.
 */

const constructed: FakeOpenCodeAdapter[] = []
let release: (() => void) | undefined

class FakeOpenCodeAdapter {
  disposed = false
  constructor() {
    constructed.push(this)
  }
  async listModels() {
    await new Promise<void>((resolve) => {
      release = resolve
    })
    return []
  }
  dispose() {
    this.disposed = true
  }
}

vi.mock('@harness/adapter-opencode', () => ({
  OpenCodeAdapter: FakeOpenCodeAdapter,
  OPENCODE_CAPABILITIES: {
    steer: false,
    fork: false,
    interrupt: true,
    reasoningItems: true,
    approvals: true,
    images: false,
  },
}))

const { providerRuntime } = await import('./adapters.js')

afterEach(() => {
  constructed.length = 0
  release = undefined
})

describe('openCodeRuntime.listModels', () => {
  it('shares one adapter run between concurrent listings', async () => {
    const runtime = providerRuntime('opencode', () => {})
    const first = runtime.listModels()
    const second = runtime.listModels()
    // A second runtime instance must join the same run too — requests from
    // different clients do not know about each other.
    const third = providerRuntime('opencode', () => {}).listModels()

    expect(constructed).toHaveLength(1)
    release?.()
    await Promise.all([first, second, third])
    expect(constructed[0]?.disposed).toBe(true)
  })

  it('runs again after the previous listing finished', async () => {
    const runtime = providerRuntime('opencode', () => {})
    const first = runtime.listModels()
    release?.()
    await first

    const second = runtime.listModels()
    expect(constructed).toHaveLength(2)
    release?.()
    await second
    expect(constructed[1]?.disposed).toBe(true)
  })
})
