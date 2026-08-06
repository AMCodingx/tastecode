import { describe, expect, it, vi } from 'vitest'
import type { McpServerConfig } from '@harness/contracts'

/**
 * MCP hot-reload under a stubbed transport. These pin the overnight fix:
 * the in-memory config and the inventory cache only change after Codex
 * actually reloaded — a failed reload must leave both describing the world
 * the running process still lives in.
 */

type Call = { method: string; params: unknown }

const fake = vi.hoisted(() => ({
  calls: [] as Call[],
  failResume: false,
}))

vi.mock('@harness/proc', () => ({
  spawnCli: vi.fn(() => ({ pid: 1 })),
  killTree: vi.fn(),
  StdioJsonRpc: class {
    onStderr(): void {}
    onNotification(): void {}
    onServerRequest(): void {}
    notify(): void {}
    dispose(): void {}

    request(method: string, params: unknown): Promise<unknown> {
      fake.calls.push({ method, params })
      if (method === 'thread/resume') {
        return fake.failResume
          ? Promise.reject(new Error('no such thread'))
          : Promise.resolve({ threadId: 't1' })
      }
      if (method === 'mcpServerStatus/list') {
        return Promise.resolve({
          data: [
            {
              name: 'github',
              serverInfo: { name: 'github', title: 'GitHub', description: null, version: '1.0' },
              authStatus: 'unsupported',
              tools: {},
              resources: [],
              resourceTemplates: [],
            },
          ],
          nextCursor: null,
        })
      }
      return Promise.resolve({})
    }
  },
}))

const { CodexAdapter } = await import('./adapter.js')

const stdioServer = (id: string): McpServerConfig => ({
  id,
  enabled: true,
  transport: { type: 'stdio', command: 'server.exe' },
})

const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

async function startedAdapter() {
  fake.calls = []
  fake.failResume = false
  const adapter = new CodexAdapter({ mcpServers: [stdioServer('github')] })
  await adapter.start()

  // Prime the per-thread inventory cache.
  await adapter.listMcpServers('t1')
  await settle()
  const cached = await adapter.listMcpServers('t1')
  expect(cached.map((server) => server.id)).toEqual(['github'])

  fake.calls = []
  return adapter
}

const listCalls = (): Call[] => fake.calls.filter((call) => call.method === 'mcpServerStatus/list')

describe('Codex MCP hot-reload', () => {
  it('reloads in order and drops the stale inventory cache on success', async () => {
    const adapter = await startedAdapter()
    const changed = vi.fn()
    adapter.on('mcpChanged', changed)

    await adapter.reloadMcpServers('t1', [stdioServer('github'), stdioServer('files')], {})

    // The running process is updated before anything local is invalidated.
    expect(fake.calls.map((call) => call.method)).toEqual([
      'thread/resume',
      'config/mcpServer/reload',
    ])
    expect(changed).toHaveBeenCalledWith({ threadId: 't1' })

    // The cache described the pre-reload world, so the next read refetches.
    await adapter.listMcpServers('t1')
    expect(listCalls()).toHaveLength(1)
  })

  it('keeps config and cache untouched when the process refuses the reload', async () => {
    const adapter = await startedAdapter()
    fake.failResume = true

    await expect(adapter.reloadMcpServers('t1', [stdioServer('files')], {})).rejects.toThrow(
      'Codex could not hot-reload MCP config; start a new session to apply it',
    )

    // No reload was issued, and the cache still answers without a refetch —
    // it matches what the running Codex actually has loaded.
    expect(fake.calls.map((call) => call.method)).toEqual(['thread/resume'])
    const cached = await adapter.listMcpServers('t1')
    expect(cached.map((server) => server.id)).toEqual(['github'])
    expect(listCalls()).toHaveLength(0)
  })

  it('refuses to hot-reload changed credentials — the child env is already fixed', async () => {
    const withSecret: McpServerConfig = {
      id: 'github',
      enabled: true,
      transport: {
        type: 'http',
        url: 'https://example.com/mcp',
        headers: { Authorization: { source: 'credential', credentialRef: 'github-token' } },
      },
    }
    fake.calls = []
    fake.failResume = false
    const adapter = new CodexAdapter({
      mcpServers: [withSecret],
      mcpCredentials: { 'github-token': 'old-secret' },
    })
    await adapter.start()

    await expect(
      adapter.reloadMcpServers('t1', [withSecret], { 'github-token': 'new-secret' }),
    ).rejects.toThrow('start a new session to apply new MCP credentials')
    expect(fake.calls.some((call) => call.method === 'thread/resume')).toBe(false)
  })
})
