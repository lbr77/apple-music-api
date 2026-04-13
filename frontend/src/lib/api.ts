export type SessionState = 'logged_out' | 'awaiting_2fa' | 'logged_in'

export interface BinaryHealth {
  path: string
  available: boolean
  version?: string | null
  error?: string | null
}

export interface HealthResponse {
  status: 'ok' | 'degraded'
  state: SessionState
  version: string
  ffmpeg: BinaryHealth
  ffprobe: BinaryHealth
}

export interface AuthResponse {
  status: 'ok' | 'need_2fa'
  state: SessionState
  message?: string
}

export interface ErrorResponse {
  status: 'error'
  state?: SessionState
  message: string
}

export class DaemonApiError extends Error {
  statusCode: number
  state?: SessionState

  constructor(statusCode: number, message: string, state?: SessionState) {
    super(message)
    this.name = 'DaemonApiError'
    this.statusCode = statusCode
    this.state = state
  }
}

async function parseResponse<T>(response: Response): Promise<T> {
  const text = await response.text()
  const payload = text.length > 0 ? JSON.parse(text) : null

  if (!response.ok) {
    const message =
      typeof payload?.message === 'string'
        ? payload.message
        : `HTTP ${response.status}`
    throw new DaemonApiError(response.status, message, payload?.state)
  }

  return payload as T
}

export async function daemonRequest<T>(
  path: string,
  token: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('Authorization', `Bearer ${token}`)

  if (init.body !== undefined && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }

  const response = await fetch(path, {
    ...init,
    headers,
  })

  return parseResponse<T>(response)
}
