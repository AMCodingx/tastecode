import { describe, expect, it } from 'vitest'
import { allowedOrigin, assertSafeBind, hasAccess } from './server.js'

describe('websocket origin gate', () => {
  it('admits our own surfaces, including non-browser clients', () => {
    // No Origin at all: the CLI, tests, a native mobile client.
    expect(allowedOrigin(undefined)).toBe(true)
    // The packaged Electron renderer loads from file:.
    expect(allowedOrigin('file://')).toBe(true)
    // The dev server and the web UI.
    expect(allowedOrigin('http://127.0.0.1:5183')).toBe(true)
    expect(allowedOrigin('http://localhost:5173')).toBe(true)
  })

  it('refuses a hostile page — loopback is not a trust boundary in a browser', () => {
    // Browsers do not apply same-origin policy to WebSocket, so without this
    // any page the user visits could drive the agent.
    expect(allowedOrigin('https://evil.example')).toBe(false)
    // The opaque origin. Any page mints one with a sandboxed iframe or a
    // data: document, so allowing it would hand the gate back to the
    // attacker — verified admitted against a live server before this fix.
    expect(allowedOrigin('null')).toBe(false)
    // Prefix tricks on our own hostnames.
    expect(allowedOrigin('http://127.0.0.1.evil.example')).toBe(false)
    expect(allowedOrigin('http://localhost.evil.example')).toBe(false)
    expect(allowedOrigin('not a url')).toBe(false)
  })
})

describe('server access token', () => {
  it('leaves the loopback server open when no token is configured', () => {
    expect(hasAccess(undefined, undefined)).toBe(true)
  })

  it('refuses an unauthenticated external bind', () => {
    expect(() => assertSafeBind('0.0.0.0', undefined)).toThrow('HARNESS_ACCESS_TOKEN')
    expect(() => assertSafeBind('192.168.1.4', '')).toThrow('HARNESS_ACCESS_TOKEN')
    expect(() => assertSafeBind('0.0.0.0', 'secret')).not.toThrow()
    expect(() => assertSafeBind('127.0.0.1', undefined)).not.toThrow()
    expect(() => assertSafeBind('::1', undefined)).not.toThrow()
  })

  it('requires the exact configured token', () => {
    expect(hasAccess('/?token=correct-token', 'correct-token')).toBe(true)
    expect(hasAccess('/?token=wrong-token', 'correct-token')).toBe(false)
    expect(hasAccess('/', 'correct-token')).toBe(false)
  })
})
