// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { App } from './App.js'

const transport = vi.hoisted(() => ({
  request: vi.fn(),
}))

vi.mock('./transport.js', () => ({
  Transport: class {
    connect() {}
    close() {}
    on() {
      return () => {}
    }
    request(method: string, params: unknown) {
      return transport.request(method, params)
    }
  },
}))

vi.mock('./ui/highlighter.js', () => ({
  warmHighlighter: () => {},
}))

vi.mock('./bridge.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./bridge.js')>()),
  isMacOS: () => true,
}))

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('harness.provider', 'codex')
  localStorage.setItem(
    'harness.projects',
    JSON.stringify([
      {
        path: '/work/project',
        sessions: [{ id: 'untouched-thread', title: 'New session', status: 'idle' }],
      },
    ]),
  )
  transport.request.mockImplementation((method: string) => {
    switch (method) {
      case 'models.list':
        return Promise.resolve({ models: [] })
      case 'workspace.info':
        return Promise.resolve({ added: 0, removed: 0, dirtyFiles: 0 })
      case 'auth.status':
        return Promise.resolve({ signedIn: true })
      case 'thread.start':
        return Promise.resolve({ threadId: 'thread-1' })
      case 'thread.sendTurn':
        return Promise.resolve({ turnId: 'turn-1' })
      default:
        return Promise.resolve({})
    }
  })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('new chats', () => {
  it('persists the macOS font smoothing setting', async () => {
    render(<App />)

    expect(document.documentElement.classList.contains('is-macos-font-smoothing')).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Account' }))
    fireEvent.click(screen.getByRole('menuitem', { name: /Settings/ }))

    const toggle = screen.getByRole('switch', { name: 'Font smoothing' })
    expect(toggle.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(toggle)

    await waitFor(() => {
      expect(localStorage.getItem('harness.macosFontSmoothing')).toBe('false')
      expect(document.documentElement.classList.contains('is-macos-font-smoothing')).toBe(false)
    })
  })

  it('switches the new chat project from the prompt', () => {
    localStorage.setItem(
      'harness.projects',
      JSON.stringify([
        { path: '/work/project', name: 'Personal Harness', sessions: [] },
        { path: '/work/another-project', name: 'Another Project', sessions: [] },
      ]),
    )

    render(<App />)

    expect(screen.getByRole('heading').textContent).toContain(
      'What should we build in Personal Harness?',
    )

    fireEvent.click(screen.getByRole('button', { name: 'Choose project' }))
    expect(screen.getAllByRole('menuitem')).toHaveLength(2)
    fireEvent.click(screen.getByRole('menuitem', { name: /Another Project/ }))

    expect(screen.getByRole('heading').textContent).toContain(
      'What should we build in Another Project?',
    )
    expect(document.querySelector('.chip__label')?.textContent).toBe('another-project')
  })

  it('keeps an untouched session out of the sidebar until the first prompt', async () => {
    render(<App />)

    const actions = document.querySelector<HTMLElement>('.rail__actions')
    expect(actions).not.toBeNull()
    fireEvent.click(within(actions!).getByRole('button', { name: 'New chat' }))

    expect(transport.request).not.toHaveBeenCalledWith('thread.start', expect.anything())
    expect(transport.request).toHaveBeenCalledWith('thread.close', {
      threadId: 'untouched-thread',
    })
    expect(document.querySelectorAll('.sessrow')).toHaveLength(0)

    const composer = document.querySelector('textarea')
    expect(composer).not.toBeNull()
    fireEvent.change(composer!, { target: { value: 'Fix the sidebar' } })
    fireEvent.keyDown(composer!, { key: 'Enter' })

    await waitFor(() => {
      expect(transport.request).toHaveBeenCalledWith('thread.start', {
        provider: 'codex',
        workspacePath: '/work/project',
        approval: 'ask',
      })
      expect(screen.getByRole('button', { name: 'Fix the sidebar' })).toBeTruthy()
    })
  })

  it('forwards model, effort, and the provider fast tier on every turn', async () => {
    transport.request.mockImplementation((method: string) => {
      switch (method) {
        case 'models.list':
          return Promise.resolve({
            models: [
              {
                id: 'gpt-5.6-sol',
                displayName: 'GPT-5.6-Sol',
                isDefault: true,
                reasoningEfforts: ['low', 'medium', 'high', 'xhigh'],
                defaultReasoningEffort: 'low',
                serviceTiers: [
                  {
                    id: 'standard',
                    name: 'Balanced',
                    description: '1x speed, standard usage',
                  },
                  {
                    id: 'priority',
                    name: 'Fast',
                    description: '1.5x speed, increased usage',
                  },
                ],
              },
            ],
          })
        case 'workspace.info':
          return Promise.resolve({ added: 0, removed: 0, dirtyFiles: 0 })
        case 'auth.status':
          return Promise.resolve({ signedIn: true })
        case 'thread.start':
          return Promise.resolve({ threadId: 'thread-1' })
        case 'thread.sendTurn':
          return Promise.resolve({ turnId: 'turn-1' })
        default:
          return Promise.resolve({})
      }
    })

    render(<App />)

    fireEvent.click(await screen.findByRole('button', { name: 'Model and reasoning' }))
    const speed = screen.getByRole('slider', { name: 'Speed' })
    vi.spyOn(speed, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      left: 100,
      top: 20,
      width: 280,
      height: 58,
      right: 380,
      bottom: 78,
      toJSON: () => ({}),
    })
    fireEvent.pointerDown(speed, { clientX: 350, pointerId: 7 })
    fireEvent.pointerUp(speed, { clientX: 350, pointerId: 7 })

    fireEvent.keyDown(screen.getByRole('slider', { name: 'Effort' }), { key: 'End' })

    const composer = screen.getByPlaceholderText('Do anything')
    fireEvent.change(composer, { target: { value: 'Use the fast lane' } })
    fireEvent.keyDown(composer, { key: 'Enter' })

    await waitFor(() => {
      expect(transport.request).toHaveBeenCalledWith('thread.start', {
        provider: 'codex',
        workspacePath: '/work/project',
        approval: 'ask',
        model: 'gpt-5.6-sol',
        effort: 'xhigh',
        serviceTier: 'priority',
      })
      expect(transport.request).toHaveBeenCalledWith('thread.sendTurn', {
        threadId: 'thread-1',
        text: 'Use the fast lane',
        model: 'gpt-5.6-sol',
        effort: 'xhigh',
        serviceTier: 'priority',
      })
    })
  })
})
