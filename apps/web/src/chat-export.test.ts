import { describe, expect, it } from 'vitest'
import type { Item } from '@harness/contracts'
import { chatToMarkdown, exportFilename } from './chat-export.js'

const item = (partial: Partial<Item>): Item => ({
  id: 'i1',
  turnId: 't1',
  type: 'message',
  status: 'completed',
  createdAt: 1,
  ...partial,
})

describe('chatToMarkdown', () => {
  it('keeps the conversation and drops reasoning and tool noise', () => {
    const markdown = chatToMarkdown('Fix the build', [
      item({ role: 'user', text: 'Why does the build fail?' }),
      item({ type: 'reasoning', text: 'Let me think about tsconfig...' }),
      item({ type: 'tool_call', text: '{"raw":"payload"}' }),
      item({ type: 'command', command: 'pnpm build', exitCode: 1 }),
      item({ type: 'file_change', path: 'tsconfig.json', linesAdded: 2, linesRemoved: 1 }),
      item({ role: 'assistant', text: 'A stale project reference. Fixed.' }),
    ])

    expect(markdown).toContain('# Fix the build')
    expect(markdown).toContain('## You\n\nWhy does the build fail?')
    expect(markdown).toContain('## Assistant\n\nA stale project reference. Fixed.')
    expect(markdown).toContain('$ pnpm build # exit 1')
    expect(markdown).toContain('> Edited `tsconfig.json` (+2 -1)')
    expect(markdown).not.toContain('tsconfig...')
    expect(markdown).not.toContain('payload')
  })

  it('skips empty messages and omits exit 0 — success is not noise', () => {
    const markdown = chatToMarkdown('Quiet', [
      item({ role: 'assistant', text: '   ' }),
      item({ type: 'command', command: 'git status' }),
      item({ type: 'command', command: 'pnpm test', exitCode: 0 }),
    ])

    expect(markdown).not.toContain('## Assistant')
    expect(markdown).toContain('$ git status\n')
    expect(markdown).toContain('$ pnpm test\n')
    expect(markdown).not.toContain('# exit')
  })
})

describe('exportFilename', () => {
  it('slugs the title and stamps the date', () => {
    expect(exportFilename('Fix: the (build)!', new Date('2026-08-06T05:00:00Z'))).toBe(
      'fix-the-build-2026-08-06.md',
    )
  })

  it('never produces an extensionless or empty name', () => {
    expect(exportFilename('!!!', new Date('2026-08-06T05:00:00Z'))).toBe('chat-2026-08-06.md')
  })
})
