export function serverUrl(baseUrl: string, hash = globalThis.location?.hash ?? ''): string {
  const token = new URLSearchParams(hash.replace(/^#/, '')).get('access_token')
  if (!token) return baseUrl

  const url = new URL(baseUrl)
  url.searchParams.set('token', token)
  return url.toString()
}
