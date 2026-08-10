import { Buffer } from 'node:buffer'
import { once } from 'node:events'
import type { IncomingMessage } from 'node:http'
import net from 'node:net'
import { describe, expect, it, vi } from 'vitest'
import { WebSocket } from 'ws'
import {
  connectionAddresses,
  listenerAddressAllowed,
  MobileAccess,
  type MobileConnectionAccess,
} from './mobile-access.js'
import { Store } from './store.js'

const INTERFACES = {
  tailscale0: [
    {
      address: '100.101.22.33',
      netmask: '255.192.0.0',
      family: 'IPv4' as const,
      mac: '00:00:00:00:00:00',
      internal: false,
      cidr: '100.101.22.33/10',
    },
  ],
  en0: [
    {
      address: '192.168.1.44',
      netmask: '255.255.255.0',
      family: 'IPv4' as const,
      mac: '00:00:00:00:00:01',
      internal: false,
      cidr: '192.168.1.44/24',
    },
    {
      address: '8.8.8.8',
      netmask: '255.255.255.0',
      family: 'IPv4' as const,
      mac: '00:00:00:00:00:02',
      internal: false,
      cidr: '8.8.8.8/24',
    },
  ],
}

describe('mobile access', () => {
  it('offers verified Tailscale routes before LAN and excludes public interfaces', () => {
    const addresses = connectionAddresses(INTERFACES, 4312, new Set(['100.101.22.33']))
    expect(addresses).toEqual([
      {
        kind: 'tailscale',
        label: 'Tailscale 100.101.22.33',
        url: 'ws://100.101.22.33:4312',
      },
      { kind: 'lan', label: 'en0 192.168.1.44', url: 'ws://192.168.1.44:4312' },
    ])
    expect(listenerAddressAllowed('::ffff:192.168.1.44', addresses)).toBe(true)
    expect(listenerAddressAllowed('8.8.8.8', addresses)).toBe(false)
  })

  it('invalidates a pairing ticket when a replacement is generated', async () => {
    const store = new Store(':memory:')
    const access = createAccess(store)
    try {
      const first = pairingTicket((await access.startPairing()).pairingUri)
      const second = pairingTicket((await access.startPairing()).pairingUri)
      expect(access.authorize(`/?pairing_ticket=${first}`)).toBeUndefined()
      expect(access.authorize(`/?pairing_ticket=${second}`)?.kind).toBe('pairing')
    } finally {
      await access.stop()
      store.close()
    }
  })

  it('exchanges one ticket for a revocable device token', async () => {
    const store = new Store(':memory:')
    const access = createAccess(store)
    try {
      const ticket = pairingTicket((await access.startPairing()).pairingUri)
      const bootstrap = access.authorize(`/?pairing_ticket=${ticket}`)
      if (!bootstrap) throw new Error('missing pairing access')

      const failedInsert = vi.spyOn(store, 'pairDevice').mockImplementationOnce(() => {
        throw new Error('database unavailable')
      })
      expect(() => access.claim(bootstrap, 'Test phone')).toThrow('database unavailable')
      failedInsert.mockRestore()
      expect(access.authorize(`/?pairing_ticket=${ticket}`)).toEqual(bootstrap)

      const claimed = access.claim(bootstrap, 'Test phone')
      expect(access.authorize(`/?pairing_ticket=${ticket}`)).toBeUndefined()
      expect(access.authorize(`/?token=${claimed.deviceToken}`)).toEqual({
        kind: 'device',
        deviceId: claimed.deviceId,
      })
      access.revoke(claimed.deviceId)
      expect(access.authorize(`/?token=${claimed.deviceToken}`)).toBeUndefined()
    } finally {
      await access.stop()
      store.close()
    }
  })

  it('disables persisted access when no private route can be advertised', async () => {
    const store = new Store(':memory:')
    store.setMobileAccessEnabled(true)
    const access = new MobileAccess({
      store,
      port: 0,
      networkInterfaces: () => ({}),
      resolveTailscaleAddresses: async () => new Set(),
      onConnection: () => undefined,
    })
    try {
      await expect(access.startPairing()).rejects.toThrow('No Tailscale or private LAN address')
      expect(store.mobileAccessEnabled()).toBe(false)
    } finally {
      await access.stop()
      store.close()
    }
  })

  it('serves the web console over HTTP only to the stable token', async () => {
    const store = new Store(':memory:')
    const access = createAccess(store, { consoleToken: 'stable-console-token' })
    await access.start()
    const port = access.status().port
    const base = `http://127.0.0.1:${port}`
    try {
      const ok = await fetch(`${base}/console?token=stable-console-token`)
      expect(ok.status).toBe(200)
      expect(ok.headers.get('cache-control')).toBe('no-store')
      expect(await ok.text()).toContain('Harness console')

      const root = await fetch(`${base}/?token=stable-console-token`)
      expect(root.status).toBe(200)

      const missing = await fetch(`${base}/console`)
      expect(missing.status).toBe(401)
      const wrong = await fetch(`${base}/console?token=wrong-token`)
      expect(wrong.status).toBe(401)
      const unknown = await fetch(`${base}/nope?token=stable-console-token`)
      expect(unknown.status).toBe(404)

      expect(access.status().consoleUrls).toEqual([
        `http://100.101.22.33:${port}/console?token=stable-console-token`,
        `http://192.168.1.44:${port}/console?token=stable-console-token`,
      ])
    } finally {
      await access.stop()
      store.close()
    }
  })

  it('accepts console sockets while devices are refused when access is off', async () => {
    const store = new Store(':memory:')
    const connections: MobileConnectionAccess[] = []
    const access = createAccess(store, {
      consoleToken: 'stable-console-token',
      onConnection: (_socket, _request, connection) => connections.push(connection),
    })
    await access.start()
    const port = access.status().port
    try {
      const offer = await access.startPairing()
      expect(offer.enabled).toBe(true)
      const claimed = access.claim(
        access.authorize(`/?pairing_ticket=${pairingTicket(offer.pairingUri)}`)!,
        'Phone',
      )

      // While enabled, a device token is accepted.
      const device = new WebSocket(
        `ws://127.0.0.1:${port}/?token=${encodeURIComponent(claimed.deviceToken)}`,
      )
      await once(device, 'open')
      device.close()
      await once(device, 'close')

      access.setProtocolEnabled(false)
      expect(access.status().enabled).toBe(false)
      expect(access.status().consoleUrls.length).toBe(2)

      // After stopping, the same device token is refused with 1008.
      const refused = new WebSocket(
        `ws://127.0.0.1:${port}/?token=${encodeURIComponent(claimed.deviceToken)}`,
      )
      const [refusedCode] = (await once(refused, 'close')) as [number, Buffer]
      expect(refusedCode).toBe(1008)

      // The console socket is unaffected.
      const consoleSocket = new WebSocket(
        `ws://127.0.0.1:${port}/ws?console_token=${encodeURIComponent('stable-console-token')}`,
      )
      await once(consoleSocket, 'open')
      expect(connections.some((connection) => connection.kind === 'console')).toBe(true)
      consoleSocket.close()
      await once(consoleSocket, 'close')
    } finally {
      await access.stop()
      store.close()
    }
  })

  it('keeps the console URL byte-identical across restarts', async () => {
    const store = new Store(':memory:')
    const port = await availablePort()
    const first = createAccess(store, { consoleToken: 'stable-console-token', port })
    await first.start()
    const firstUrls = first.status().consoleUrls
    await first.stop()

    const second = createAccess(store, { consoleToken: 'stable-console-token', port })
    await second.start()
    try {
      expect(second.status().consoleUrls).toEqual(firstUrls)
      expect(second.status().port).toBe(port)
    } finally {
      await second.stop()
      store.close()
    }
  })
})

function createAccess(
  store: Store,
  options: {
    consoleToken?: string
    port?: number
    onConnection?: (
      socket: WebSocket,
      request: IncomingMessage,
      access: MobileConnectionAccess,
    ) => void
  } = {},
): MobileAccess {
  return new MobileAccess({
    store,
    port: options.port ?? 0,
    serverName: 'Test computer',
    networkInterfaces: () => INTERFACES,
    resolveTailscaleAddresses: async () => new Set(['100.101.22.33']),
    consoleToken: options.consoleToken,
    onConnection: options.onConnection ?? (() => undefined),
  })
}

async function availablePort(): Promise<number> {
  const server = net.createServer()
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('could not reserve test port')
  await new Promise<void>((resolve) => server.close(() => resolve()))
  return address.port
}

function pairingTicket(uri: string): string {
  const encoded = new URL(uri).searchParams.get('payload')
  if (!encoded) throw new Error('missing pairing payload')
  const payload = JSON.parse(Buffer.from(encoded, 'base64url').toString()) as { ticket?: unknown }
  if (typeof payload.ticket !== 'string') throw new Error('missing pairing ticket')
  return payload.ticket
}
