import { describe, expect, it } from 'vitest'
import { hasAccess } from './server.js'

describe('server access token', () => {
  it('leaves the loopback server open when no token is configured', () => {
    expect(hasAccess(undefined, undefined)).toBe(true)
  })

  it('requires the exact configured token', () => {
    expect(hasAccess('/?token=correct-token', 'correct-token')).toBe(true)
    expect(hasAccess('/?token=wrong-token', 'correct-token')).toBe(false)
    expect(hasAccess('/', 'correct-token')).toBe(false)
  })
})
