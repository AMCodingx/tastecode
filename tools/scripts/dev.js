/**
 * One command to run the whole thing: core server, Vite, and Electron.
 *
 * Node rather than a shell script on purpose — we are a Windows + macOS team
 * and a .sh here would break one of us. See rules/code.md.
 */
import { execFile, spawn } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import path from 'node:path'
import net from 'node:net'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const VITE_URL = 'http://127.0.0.1:5183'
const isWin = process.platform === 'win32'
const mobile = process.argv.includes('--mobile')
const children = []
const execFileAsync = promisify(execFile)
let shuttingDown = false

function run(name, packageDir, args, env = {}) {
  // npm/pnpm shims are .cmd files on Windows, which must go through cmd.exe.
  const child = isWin
    ? spawn('cmd.exe', ['/d', '/s', '/c', 'pnpm', ...args], {
        cwd: path.join(root, packageDir),
        env: { ...process.env, ...env },
        stdio: 'pipe',
      })
    : spawn('pnpm', args, {
        cwd: path.join(root, packageDir),
        env: { ...process.env, ...env },
        stdio: 'pipe',
      })

  const prefix = `[${name}]`
  child.stdout.on('data', (d) => process.stdout.write(prefixLines(prefix, d.toString())))
  child.stderr.on('data', (d) => process.stderr.write(prefixLines(prefix, d.toString())))
  child.on('exit', (code, signal) => {
    if (shuttingDown) return
    const reason = code === null ? `signal ${signal ?? 'unknown'}` : `code ${code}`
    console.error(`${prefix} exited with ${reason}`)
    shutdown(code ?? 1)
  })
  children.push(child)
  return child
}

function prefixLines(prefix, text) {
  return text
    .split('\n')
    .filter((line) => line !== '')
    .map((line) => `${prefix} ${line}\n`)
    .join('')
}

function waitForPort(port, host = '127.0.0.1', timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.connect(port, host)
      socket.once('connect', () => {
        socket.destroy()
        resolve()
      })
      socket.once('error', () => {
        socket.destroy()
        if (Date.now() > deadline) reject(new Error(`port ${port} never opened`))
        else setTimeout(attempt, 200)
      })
    }
    attempt()
  })
}

function portIsOpen(port, host) {
  return new Promise((resolve) => {
    const socket = net.connect(port, host)
    let settled = false
    const finish = (open) => {
      if (settled) return
      settled = true
      socket.destroy()
      resolve(open)
    }
    socket.once('connect', () => finish(true))
    socket.once('error', () => finish(false))
    socket.setTimeout(1_000, () => finish(false))
  })
}

async function requireFreePorts(host) {
  const ports = [4311, 5183]
  const states = await Promise.all(ports.map((port) => portIsOpen(port, host)))
  const occupied = ports.filter((_, index) => states[index])
  if (occupied.length === 0) return

  console.error(
    `[dev] port${occupied.length === 1 ? '' : 's'} ${occupied.join(', ')} already in use. ` +
      'Stop the existing Personal Harness dev process before starting another one.',
  )
  process.exit(1)
}

async function tailscaleIPv4() {
  const candidates = ['tailscale']
  if (process.platform === 'darwin') {
    candidates.push('/Applications/Tailscale.app/Contents/MacOS/Tailscale')
  }
  if (process.platform === 'win32' && process.env['ProgramFiles']) {
    candidates.push(path.join(process.env['ProgramFiles'], 'Tailscale', 'tailscale.exe'))
  }

  for (const command of candidates) {
    try {
      const { stdout } = await execFileAsync(command, ['ip', '-4'])
      const address = stdout.trim().split(/\s+/)[0]
      if (address && net.isIPv4(address)) return address
    } catch {
      // Try the next normal installation location.
    }
  }

  throw new Error(
    'Tailscale is not running, or its CLI could not be found. Open Tailscale and try again.',
  )
}

function shutdown(exitCode = 0) {
  if (shuttingDown) return
  shuttingDown = true
  for (const child of children) child.kill()
  process.exit(exitCode)
}
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)

if (mobile) {
  const host = await tailscaleIPv4()
  await requireFreePorts(host)
  const accessToken = randomBytes(24).toString('base64url')
  const serverUrl = `ws://${host}:4311`
  const webUrl = `http://${host}:5183/#access_token=${accessToken}`

  run('server', 'apps/server', ['run', 'dev'], {
    HARNESS_HOST: host,
    HARNESS_ACCESS_TOKEN: accessToken,
  })
  run('web', 'apps/web', ['run', 'dev', '--host', host], {
    VITE_HARNESS_SERVER_URL: serverUrl,
  })

  await Promise.all([waitForPort(4311, host), waitForPort(5183, host)])
  console.log(`\nOpen on your Tailscale-connected phone:\n${webUrl}\n`)
} else {
  await requireFreePorts('127.0.0.1')
  run('server', 'apps/server', ['run', 'dev'])
  run('web', 'apps/web', ['run', 'dev'])

  // Electron must not load before Vite is serving, or it shows a blank window
  // and the user thinks the app is broken.
  await waitForPort(5183)
  run('desktop', 'apps/desktop', ['run', 'start'], { HARNESS_DEV_SERVER: VITE_URL })
}
