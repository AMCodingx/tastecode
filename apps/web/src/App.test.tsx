// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { DomainEvent } from '@harness/contracts'
import { App } from './App.js'

const transport = vi.hoisted(() => ({
  request: vi.fn(),
  listeners: new Map<string, (data: unknown) => void>(),
}))

vi.mock('./transport.js', () => ({
  Transport: class {
    connect() {}
    close() {}
    on(channel: string, listener: (data: unknown) => void) {
      transport.listeners.set(channel, listener)
      return () => {
        transport.listeners.delete(channel)
      }
    }
    request(method: string, params: unknown) {
      return transport.request(method, params)
    }
  },
}))

vi.mock('./ui/highlighter.js', () => ({
  warmHighlighter: () => {},
}))

// App tests exercise session routing, while Thread's own tests cover its
// virtualized renderer. happy-dom intentionally renders no virtual rows.
vi.mock('./ui/Thread.js', () => ({
  Thread: (props: { items: { id: string; text?: string }[] }) => (
    <div data-testid="thread">
      {props.items.map((item) => (
        <span key={item.id}>{item.text}</span>
      ))}
    </div>
  ),
}))

vi.mock('./bridge.js', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./bridge.js')>()),
  isMacOS: () => true,
}))

beforeEach(() => {
  transport.listeners.clear()
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
    fireEvent.click(screen.getByRole('button', { name: 'Enable fast mode' }))
    fireEvent.keyDown(screen.getByRole('slider', { name: 'Reasoning effort' }), { key: 'End' })

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

describe('sidebar chat ordering', () => {
  it('persists the order chosen by dragging a chat row', async () => {
    localStorage.setItem(
      'harness.projects',
      JSON.stringify([
        {
          path: '/work/project',
          sessions: [
            { id: 'thread-1', title: 'First chat', status: 'idle' },
            { id: 'thread-2', title: 'Second chat', status: 'idle' },
            { id: 'thread-3', title: 'Third chat', status: 'idle' },
          ],
        },
      ]),
    )

    render(<App />)

    const source = screen.getByRole('button', { name: 'First chat' }).closest('li')!
    const target = screen.getByRole('button', { name: 'Third chat' }).closest('li')!
    vi.spyOn(target, 'getBoundingClientRect').mockReturnValue({
      bottom: 88,
      height: 28,
      left: 0,
      right: 200,
      top: 60,
      width: 200,
      x: 0,
      y: 60,
      toJSON: () => ({}),
    })
    const dataTransfer = {
      dropEffect: 'none',
      effectAllowed: 'none',
      setData: vi.fn(),
    }

    fireEvent.dragStart(source, { dataTransfer })
    fireEvent.dragOver(target, { clientY: 80, dataTransfer })
    fireEvent.drop(target, { clientY: 80, dataTransfer })

    await waitFor(() => {
      const projects = JSON.parse(localStorage.getItem('harness.projects') ?? '[]') as {
        sessions: { id: string }[]
      }[]
      expect(projects[0]?.sessions.map((session) => session.id)).toEqual([
        'thread-3',
        'thread-1',
        'thread-2',
      ])
    })
  })
})

describe('global shortcuts', () => {
  it('opens a searchable palette for actions, projects, and chats', () => {
    localStorage.setItem(
      'harness.projects',
      JSON.stringify([
        {
          path: '/work/project',
          name: 'Personal Harness',
          sessions: [{ id: 'thread-1', title: 'Fix keyboard flow', status: 'idle' }],
        },
        {
          path: '/work/another-project',
          name: 'Another Project',
          sessions: [{ id: 'thread-2', title: 'Polish the sidebar', status: 'idle' }],
        },
      ]),
    )

    render(<App />)
    fireEvent.keyDown(window, { key: 'k', metaKey: true })

    expect(screen.getByRole('dialog', { name: 'Command palette' })).toBeTruthy()
    const search = screen.getByRole('textbox', { name: 'Search commands' })
    expect(document.activeElement).toBe(search)
    expect(screen.getByRole('option', { name: /Settings/ })).toBeTruthy()
    expect(
      screen.getByRole('option', { name: /^Another Project \/work\/another-project$/ }),
    ).toBeTruthy()
    expect(screen.getByRole('option', { name: /Polish the sidebar/ })).toBeTruthy()

    fireEvent.change(search, { target: { value: 'polish sidebar' } })
    fireEvent.keyDown(search, { key: 'Enter' })

    expect(screen.queryByRole('dialog', { name: 'Command palette' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Polish the sidebar' }).classList).toContain(
      'is-active',
    )
  })

  it('opens the project switcher directly and shows shortcuts beside matching actions', () => {
    render(<App />)

    const actions = document.querySelector<HTMLElement>('.rail__actions')
    expect(actions).not.toBeNull()
    expect(within(actions!).getByText('⌘N')).toBeTruthy()
    expect(within(actions!).getByText('⌘⇧O')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Project' }).textContent).toContain('⌘P')

    fireEvent.keyDown(window, { key: 'p', metaKey: true })

    expect(screen.getByRole('dialog', { name: 'Switch project' })).toBeTruthy()
    expect(screen.getAllByRole('option')).toHaveLength(2)
    expect(screen.queryByRole('option', { name: /New session/ })).toBeNull()
  })

  it('runs common shortcuts and never intercepts them from the composer', () => {
    render(<App />)

    const composer = screen.getByPlaceholderText('Do anything')
    fireEvent.change(composer, { target: { value: 'Keep this draft intact' } })
    fireEvent.keyDown(composer, { key: 'n', metaKey: true })
    fireEvent.keyDown(composer, { key: 'k', metaKey: true })
    fireEvent.keyDown(composer, { key: ',', metaKey: true })

    expect(transport.request).not.toHaveBeenCalledWith('thread.close', {
      threadId: 'untouched-thread',
    })
    expect(screen.queryByRole('dialog', { name: 'Command palette' })).toBeNull()
    expect(screen.queryByRole('dialog', { name: 'Settings' })).toBeNull()
    expect((composer as HTMLTextAreaElement).value).toBe('Keep this draft intact')

    composer.blur()
    fireEvent.keyDown(window, { key: 'n', metaKey: true })
    expect(transport.request).toHaveBeenCalledWith('thread.close', {
      threadId: 'untouched-thread',
    })

    fireEvent.keyDown(window, { key: 'l', metaKey: true })
    expect(document.activeElement).toBe(composer)

    composer.blur()
    fireEvent.keyDown(window, { key: ',', metaKey: true })
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeTruthy()
  })
})

describe('live sessions', () => {
  it('shows the most recently active session first', () => {
    localStorage.setItem(
      'harness.projects',
      JSON.stringify([
        {
          path: '/work/project',
          sessions: [
            { id: 'thread-1', title: 'Older session', status: 'idle' },
            { id: 'thread-2', title: 'Newer session', status: 'idle' },
          ],
        },
      ]),
    )

    render(<App />)

    expect(sessionTitles()).toEqual(['Newer session', 'Older session'])

    emitThreadEvent('thread-1', {
      type: 'turn.started',
      turn: { id: 'turn-1', threadId: 'thread-1', status: 'running', createdAt: 0 },
    })

    expect(sessionTitles()).toEqual(['Older session', 'Newer session'])
  })

  it('keeps background session state and distinguishes work from attention', async () => {
    localStorage.setItem(
      'harness.projects',
      JSON.stringify([
        {
          path: '/work/project',
          sessions: [
            { id: 'thread-1', title: 'First session', status: 'idle' },
            { id: 'thread-2', title: 'Second session', status: 'idle' },
          ],
        },
      ]),
    )

    render(<App />)

    fireEvent.click(screen.getByRole('button', { name: 'First session' }))
    emitThreadEvent('thread-1', {
      type: 'turn.started',
      turn: { id: 'turn-1', threadId: 'thread-1', status: 'running', createdAt: 0 },
    })
    emitThreadEvent('thread-1', {
      type: 'item.started',
      item: {
        id: 'item-1',
        turnId: 'turn-1',
        type: 'message',
        role: 'assistant',
        status: 'started',
        text: 'First result',
        createdAt: 0,
      },
    })

    const working = screen.getByRole('button', { name: 'First session, working' })
    expect(working.querySelector('.sess__spinner')?.textContent).toBe('⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏')

    fireEvent.click(screen.getByRole('button', { name: 'Second session' }))
    emitThreadEvent('thread-2', {
      type: 'turn.started',
      turn: { id: 'turn-2', threadId: 'thread-2', status: 'running', createdAt: 0 },
    })
    emitThreadEvent('thread-2', {
      type: 'approval.requested',
      request: {
        id: 'approval-1',
        kind: 'command',
        command: 'pnpm test',
        createdAt: 0,
      },
    })

    const attention = screen.getByRole('button', { name: 'Second session, needs attention' })
    expect(attention.querySelector('.sess__status-dot.is-attention')).not.toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'First session, working' }))
    await waitFor(() => expect(screen.getByText('First result')).toBeTruthy())

    emitThreadEvent('thread-1', {
      type: 'turn.completed',
      turnId: 'turn-1',
      status: 'completed',
    })
    expect(screen.getByRole('button', { name: 'First session' })).toBeTruthy()
  })
})

function emitThreadEvent(threadId: string, event: DomainEvent) {
  act(() => {
    transport.listeners.get('thread.event')?.({ threadId, event })
  })
}

function sessionTitles(): string[] {
  return Array.from(document.querySelectorAll('.sess__title'), (node) => node.textContent ?? '')
}
