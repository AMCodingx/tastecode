/**
 * One command to run the whole thing: core server, Vite, and Electron.
 *
 * Node rather than a shell script on purpose — we are a Windows + macOS team
 * and a .sh here would break one of us. See rules/code.md.
 */
import { execFile, spawn } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { existsSync, readdirSync, statSync } from 'node:fs'
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
const launcherId = createHash('sha256').update(root).digest('hex')
const launcherControlPort = 20_000 + (Number.parseInt(launcherId.slice(0, 4), 16) % 20_000)
let shuttingDown = false
let launcherControlServer

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
        // The wrapper leads a process group so shutdown reaches tsx, Vite,
        // Electron, and their descendants instead of orphaning them.
        detached: true,
      })

  const prefix = `[${name}]`
  child.stdout.on('data', (d) => {
    if (!shuttingDown) process.stdout.write(prefixLines(prefix, d.toString()))
  })
  child.stderr.on('data', (d) => {
    if (!shuttingDown) process.stderr.write(prefixLines(prefix, d.toString()))
  })
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

async function listenerPids(port) {
  if (isWin) {
    const { stdout } = await execFileAsync('netstat.exe', ['-ano', '-p', 'tcp'])
    return stdout
      .split(/\r?\n/)
      .map((line) => line.trim().split(/\s+/))
      .filter(
        (fields) =>
          fields.length >= 5 &&
          fields[0].toUpperCase() === 'TCP' &&
          fields[1].endsWith(`:${port}`) &&
          fields[3].toUpperCase() === 'LISTENING',
      )
      .map((fields) => Number.parseInt(fields[4], 10))
      .filter((pid) => Number.isInteger(pid) && pid > 0)
  }

  try {
    const { stdout } = await execFileAsync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN', '-t'])
    return stdout
      .trim()
      .split(/\s+/)
      .map((pid) => Number.parseInt(pid, 10))
      .filter((pid) => Number.isInteger(pid) && pid > 0)
  } catch (error) {
    if (error?.code === 1) return []
    throw error
  }
}

function processExists(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    if (error?.code === 'ESRCH') return false
    if (error?.code === 'EPERM') return true
    throw error
  }
}

function processGroupExists(groupId) {
  try {
    process.kill(-groupId, 0)
    return true
  } catch (error) {
    if (error?.code === 'ESRCH') return false
    if (error?.code === 'EPERM') return true
    throw error
  }
}

function signalProcessGroup(groupId, signal) {
  try {
    process.kill(-groupId, signal)
  } catch (error) {
    if (error?.code !== 'ESRCH') throw error
  }
}

async function waitForProcessGroupToExit(groupId, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  while (processGroupExists(groupId) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 50))
  }
  return !processGroupExists(groupId)
}

async function stopPortOwner(pid, force = false) {
  if (pid === process.pid) throw new Error('dev launcher unexpectedly owns an application port')

  if (isWin) {
    try {
      await execFileAsync('taskkill.exe', ['/pid', String(pid), '/T', '/F'], {
        windowsHide: true,
      })
    } catch (error) {
      if (processExists(pid)) throw error
    }
    return
  }

  try {
    process.kill(pid, force ? 'SIGKILL' : 'SIGTERM')
  } catch (error) {
    if (error?.code !== 'ESRCH') throw error
  }
}

async function waitForPortsToClose(ports, host, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs
  let occupied = ports
  while (occupied.length > 0 && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 100))
    const states = await Promise.all(occupied.map((port) => portIsOpen(port, host)))
    occupied = occupied.filter((_, index) => states[index])
  }
  return occupied
}

async function reclaimOccupiedPorts(host) {
  const ports = [4311, 5183]
  const states = await Promise.all(ports.map((port) => portIsOpen(port, host)))
  const occupied = ports.filter((_, index) => states[index])
  if (occupied.length === 0) return

  const owners = new Set((await Promise.all(occupied.map(listenerPids))).flat())
  if (owners.size === 0) {
    const currentStates = await Promise.all(occupied.map((port) => portIsOpen(port, host)))
    const remaining = occupied.filter((_, index) => currentStates[index])
    if (remaining.length === 0) return
    throw new Error(
      `port${remaining.length === 1 ? '' : 's'} ${remaining.join(', ')} ` +
        'still in use, but the owning process could not be identified',
    )
  }

  console.log(
    `[dev] port${occupied.length === 1 ? '' : 's'} ${occupied.join(', ')} already in use; ` +
      `stopping process${owners.size === 1 ? '' : 'es'} ${[...owners].join(', ')}`,
  )
  await Promise.all([...owners].map(stopPortOwner))

  let remaining = await waitForPortsToClose(occupied, host, 2_000)
  if (remaining.length > 0) {
    const stubbornOwners = new Set((await Promise.all(remaining.map(listenerPids))).flat())
    await Promise.all([...stubbornOwners].map((pid) => stopPortOwner(pid, true)))
    remaining = await waitForPortsToClose(remaining, host, 3_000)
  }

  if (remaining.length > 0) {
    throw new Error(
      `port${remaining.length === 1 ? '' : 's'} ${remaining.join(', ')} ` +
        'did not close after stopping the owning process',
    )
  }
}

function requestLauncherShutdown() {
  return new Promise((resolve, reject) => {
    const socket = net.connect(launcherControlPort, '127.0.0.1')
    let response = ''
    let settled = false
    const timeout = setTimeout(() => finish(new Error('shutdown request timed out')), 3_000)

    const finish = (error) => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      socket.destroy()
      if (error) reject(error)
      else resolve()
    }

    socket.setEncoding('utf8')
    socket.once('connect', () => socket.end(`stop:${launcherId}\n`))
    socket.on('data', (chunk) => {
      response += chunk
    })
    socket.once('end', () => {
      if (response.trim() === `stopping:${launcherId}`) finish()
      else finish(new Error('shutdown request was rejected'))
    })
    socket.once('error', finish)
  })
}

function createLauncherControlServer() {
  const server = net.createServer((socket) => {
    socket.setEncoding('utf8')
    let command = ''
    socket.on('data', (chunk) => {
      command += chunk
      if (command.length > 1_000) socket.destroy()
    })
    socket.on('end', () => {
      if (command.trim() !== `stop:${launcherId}`) {
        socket.end('denied')
        return
      }
      socket.end(`stopping:${launcherId}`, () => void shutdown(0))
    })
  })
  return server
}

async function claimLauncher() {
  while (true) {
    const server = createLauncherControlServer()
    try {
      await new Promise((resolve, reject) => {
        server.once('error', reject)
        server.listen(launcherControlPort, '127.0.0.1', resolve)
      })
      launcherControlServer = server
      return
    } catch (error) {
      if (error?.code !== 'EADDRINUSE') throw error
    }

    try {
      await requestLauncherShutdown()
    } catch (error) {
      throw new Error(`launcher control port is owned by another application: ${error.message}`)
    }
    console.log('[dev] stopping previous Personal Harness dev process')
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
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

async function stopChild(child) {
  if (!child.pid) return

  if (isWin) {
    if (child.exitCode !== null || child.signalCode !== null) return
    await new Promise((resolve) => {
      let settled = false
      const finish = () => {
        if (settled) return
        settled = true
        resolve()
      }
      const killer = spawn('taskkill.exe', ['/pid', String(child.pid), '/T', '/F'], {
        stdio: 'ignore',
        windowsHide: true,
      })
      killer.once('error', finish)
      killer.once('exit', finish)
      setTimeout(finish, 5_000).unref()
    })
    return
  }

  signalProcessGroup(child.pid, 'SIGTERM')
  if (await waitForProcessGroupToExit(child.pid, 3_000)) return

  signalProcessGroup(child.pid, 'SIGKILL')
  await waitForProcessGroupToExit(child.pid, 2_000)
}

async function webDistIsStale() {
  const webRoot = path.join(root, 'apps/web')
  const distIndex = path.join(webRoot, 'dist/index.html')
  if (!existsSync(distIndex)) return true
  const newestSource = await newestMtime(path.join(webRoot, 'src'))
  if (newestSource === undefined) return false
  return statSync(distIndex).mtimeMs < newestSource
}

async function newestMtime(dir) {
  let newest
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      const inner = await newestMtime(full)
      if (inner !== undefined && (newest === undefined || inner > newest)) newest = inner
    } else if (entry.isFile()) {
      const mtime = statSync(full).mtimeMs
      if (newest === undefined || mtime > newest) newest = mtime
    }
  }
  return newest
}

async function runToCompletion(name, packageDir, args) {
  await new Promise((resolve, reject) => {
    const child = isWin
      ? spawn('cmd.exe', ['/d', '/s', '/c', 'pnpm', ...args], {
          cwd: path.join(root, packageDir),
          stdio: 'inherit',
        })
      : spawn('pnpm', args, {
          cwd: path.join(root, packageDir),
          stdio: 'inherit',
        })
    child.once('error', reject)
    child.once('exit', (code) =>
      code === 0 ? resolve() : reject(new Error(`${name} build failed (${code})`)),
    )
  })
}

async function shutdown(exitCode = 0) {
  if (shuttingDown) return
  shuttingDown = true
  launcherControlServer?.close()
  await Promise.all(children.map(stopChild))
  process.exit(exitCode)
}
process.on('SIGINT', () => void shutdown(0))
process.on('SIGTERM', () => void shutdown(0))

await claimLauncher()

// The server serves the built web app to phones on the mobile port, so a
// stale apps/web/dist would silently hand the phone an old UI. Rebuild when
// the sources are newer than the last build; skip the work when fresh.
if (await webDistIsStale()) {
  console.log('[dev] web app build is stale — rebuilding so phones get the current UI')
  await runToCompletion('web', 'apps/web', ['run', 'build'])
}

if (mobile) {
  const host = await tailscaleIPv4()
  await reclaimOccupiedPorts(host)
  const accessToken = randomBytes(24).toString('base64url')
  const serverUrl = `ws://${host}:4311`
  const webUrl = `http://${host}:5183/#access_token=${accessToken}`

  run('server', 'apps/server', ['run', 'dev'], {
    HARNESS_HOST: host,
    HARNESS_ACCESS_TOKEN: accessToken,
    // The phone's web-app surface (mobile listener) serves the live Vite
    // source in dev, not a stale build.
    HARNESS_WEB_DEV_SERVER: `http://${host}:5183`,
  })
  run('web', 'apps/web', ['run', 'dev', '--host', host], {
    VITE_HARNESS_SERVER_URL: serverUrl,
  })

  await Promise.all([waitForPort(4311, host), waitForPort(5183, host)])
  console.log(`\nOpen on your Tailscale-connected phone:\n${webUrl}\n`)
} else {
  await reclaimOccupiedPorts('127.0.0.1')
  run('server', 'apps/server', ['run', 'dev'], {
    // Same as the --mobile flow: the phone web app served by this dev stack
    // proxies to Vite so it tracks source edits without rebuilding dist.
    HARNESS_WEB_DEV_SERVER: VITE_URL,
  })
  run('web', 'apps/web', ['run', 'dev'])

  // Electron must not load before Vite is serving, or it shows a blank window
  // and the user thinks the app is broken.
  await waitForPort(5183)
  run('desktop', 'apps/desktop', ['run', 'start'], { HARNESS_DEV_SERVER: VITE_URL })
}
