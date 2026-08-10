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
const CONSOLE_REFERENCE = 'mobile-console-token'
const WEB_REFERENCE = 'mobile-web-token'

export function loadOrCreateConsoleToken(): string {
  return loadOrCreateStableToken(CONSOLE_REFERENCE)
}

/**
 * The long-lived token that authenticates the full web app on a phone
 * (`http://<address>:<port>/#access_token=<this>`). Separate from the console
 * token on purpose: this one grants full admin access, so it must not be the
 * same credential that a quick device-management page carries.
 */
export function loadOrCreateWebClientToken(): string {
  return loadOrCreateStableToken(WEB_REFERENCE)
}

function loadOrCreateStableToken(reference: string): string {
  try {
    return readCredential(reference)
  } catch {
    // First run, or the credential was removed — create one below.
  }
  const token = randomBytes(32).toString('base64url')
  try {
    writeCredential(reference, token)
  } catch {
    // No keychain (unusual for the desktop hosts). The surface is simply not
    // offered; the caller logs the degradation.
    return ''
  }
  return token
}

/** Testing and reset support: forget a stored token so a new one is minted. */
export function clearConsoleToken(): void {
  clearStableToken(CONSOLE_REFERENCE)
}

/** Testing and reset support: forget a stored token so a new one is minted. */
export function clearWebClientToken(): void {
  clearStableToken(WEB_REFERENCE)
}

function clearStableToken(reference: string): void {
  if (hasCredential(reference)) removeCredential(reference)
}
