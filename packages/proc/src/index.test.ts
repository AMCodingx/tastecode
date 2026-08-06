import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { killTree, runCli, spawnCli } from './index.js'

describe('runCli', () => {
  it('captures a short command without invoking a platform shell directly', async () => {
    const result = await runCli('node', ['--version'])
    expect(result.code).toBe(0)
    expect(result.stdout).toMatch(/^v\d+\./)
  })
})

describe('spawnCli', () => {
  it('can replace the inherited environment for untrusted commands', async () => {
    process.env['HARNESS_HIDDEN'] = 'secret'
    try {
      const child = spawnCli(
        'node',
        [
          '-e',
          'process.stdout.write(`${process.env.HARNESS_VISIBLE}|${process.env.HARNESS_HIDDEN ?? ""}`)',
        ],
        {
          replaceEnv: true,
          env: {
            PATH: process.env['PATH'],
            PATHEXT: process.env['PATHEXT'],
            SYSTEMROOT: process.env['SYSTEMROOT'],
            COMSPEC: process.env['COMSPEC'],
            HARNESS_VISIBLE: 'yes',
          },
        },
      )
      let output = ''
      child.stdout.setEncoding('utf8')
      child.stdout.on('data', (chunk: string) => (output += chunk))
      const code = await new Promise<number | null>((resolve, reject) => {
        child.on('error', reject)
        child.on('exit', resolve)
      })
      expect(code).toBe(0)
      expect(output).toBe('yes|')
    } finally {
      delete process.env['HARNESS_HIDDEN']
    }
  })
})

describe('killTree', () => {
  it('kills the real process behind the shim, not only the shim', async () => {
    // The grandchild heartbeats into a temp file; if only the cmd.exe shim
    // died (the pre-fix Windows behavior), the heartbeat keeps ticking.
    const beat = path.join(os.tmpdir(), `harness-killtree-${Date.now()}.txt`)
    const script = `const fs=require('fs');setInterval(()=>fs.writeFileSync(${JSON.stringify(
      beat,
    )},String(Date.now())),150)`
    const child = spawnCli('node', ['-e', script])
    await waitFor(() => existsSync(beat), 5_000)

    killTree(child)
    await sleep(700)
    const afterKill = readFileSync(beat, 'utf8')
    await sleep(700)
    expect(readFileSync(beat, 'utf8')).toBe(afterKill)
    rmSync(beat, { force: true })
  })
})

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

async function waitFor(check: () => boolean, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs
  while (!check()) {
    if (Date.now() > deadline) throw new Error('condition never became true')
    await sleep(100)
  }
}
