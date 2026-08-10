import { randomBytes } from 'node:crypto'
import { hasCredential, readCredential, removeCredential, writeCredential } from './credentials.js'

/**
 * The long-lived token that keeps the web-console URL stable across restarts.
 *
 * The console URL is `http://<address>:<port>/console?token=<this>`; because
 * the port is fixed and the Tailscale address is stable, only the token can
 * change — and it never does, because it lives in the OS credential store
 * rather than in memory or in the database (the DB holds no credentials by
 * policy). The user bookmarks the URL once and it keeps working.
 */
const REFERENCE = 'mobile-console-token'

export function loadOrCreateConsoleToken(): string {
  try {
    return readCredential(REFERENCE)
  } catch {
    // First run, or the credential was removed — create one below.
  }
  const token = randomBytes(32).toString('base64url')
  try {
    writeCredential(REFERENCE, token)
  } catch {
    // No keychain (unusual for the desktop hosts). The console is simply not
    // offered; the caller logs the degradation.
    return ''
  }
  return token
}

/** Testing and reset support: forget the stored token so a new one is minted. */
export function clearConsoleToken(): void {
  if (hasCredential(REFERENCE)) removeCredential(REFERENCE)
}
