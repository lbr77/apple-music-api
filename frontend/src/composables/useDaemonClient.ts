import { useDocumentVisibility, useStorage } from '@vueuse/core'
import { computed, shallowRef, watch } from 'vue'

import {
  DaemonApiError,
  daemonRequest,
  type AuthResponse,
  type HealthResponse,
  type SessionState,
} from '@/lib/api'

export type ActionName =
  | 'verify'
  | 'refresh'
  | 'login'
  | 'two_factor'
  | 'reset'
  | 'logout'

export interface Notice {
  tone: 'default' | 'destructive'
  title: string
  description: string
}

interface CredentialsPayload {
  username: string
  password: string
}

const DEFAULT_NOTICE: Notice = {
  tone: 'default',
  title: 'Bearer token required',
  description: 'Enter the daemon Bearer token before managing the Apple Music account.',
}

export function useDaemonClient() {
  const token = useStorage('wrapper.frontend.api-token', '')
  const activeTab = useStorage<'access' | 'account'>(
    'wrapper.frontend.active-tab',
    'access',
  )
  const sessionState = shallowRef<SessionState>('logged_out')
  const health = shallowRef<HealthResponse | null>(null)
  const notice = shallowRef<Notice>(DEFAULT_NOTICE)
  const actionInFlight = shallowRef<ActionName | null>(null)
  const initialized = shallowRef(false)
  const documentVisibility = useDocumentVisibility()

  const tokenValue = computed({
    get: () => token.value,
    set: (value: string) => {
      token.value = value
    },
  })

  const authorized = computed(() => health.value !== null)
  const awaitingTwoFactor = computed(
    () => sessionState.value === 'awaiting_2fa',
  )

  watch(awaitingTwoFactor, (pending) => {
    if (pending) {
      activeTab.value = 'account'
    }
  })

  watch(documentVisibility, async (visibility) => {
    if (
      visibility === 'visible' &&
      authorized.value &&
      actionInFlight.value === null
    ) {
      try {
        await refreshState('refresh')
      } catch {}
    }
  })

  function resetAuthorizationNotice() {
    notice.value = DEFAULT_NOTICE
  }

  function handleError(error: unknown) {
    if (error instanceof DaemonApiError) {
      if (error.statusCode === 401) {
        health.value = null
        sessionState.value = 'logged_out'
        activeTab.value = 'access'
        notice.value = {
          tone: 'destructive',
          title: 'Bearer token rejected',
          description: error.message,
        }
        return
      }

      if (error.state !== undefined) {
        sessionState.value = error.state
      }

      notice.value = {
        tone: 'destructive',
        title: 'Request failed',
        description: error.message,
      }
      return
    }

    notice.value = {
      tone: 'destructive',
      title: 'Unexpected error',
      description: error instanceof Error ? error.message : String(error),
    }
  }

  async function runAction<T>(
    action: ActionName,
    task: () => Promise<T>,
  ): Promise<T | undefined> {
    if (actionInFlight.value !== null) {
      return undefined
    }

    actionInFlight.value = action

    try {
      return await task()
    } catch (error) {
      handleError(error)
      return undefined
    } finally {
      actionInFlight.value = null
    }
  }

  async function refreshState(action: ActionName = 'refresh') {
    const trimmedToken = token.value.trim()

    if (!trimmedToken) {
      health.value = null
      sessionState.value = 'logged_out'
      activeTab.value = 'access'
      resetAuthorizationNotice()
      return
    }

    const result = await runAction(action, async () => {
      const [status, healthReport] = await Promise.all([
        daemonRequest<AuthResponse>('/status', trimmedToken),
        daemonRequest<HealthResponse>('/health', trimmedToken),
      ])

      sessionState.value = status.state
      health.value = healthReport
      return { status, healthReport }
    })

    return result
  }

  async function verifyToken(options: { silentSuccess?: boolean } = {}) {
    const trimmedToken = token.value.trim()

    if (!trimmedToken) {
      notice.value = {
        tone: 'destructive',
        title: 'Missing token',
        description: 'Enter the daemon Bearer token before you continue.',
      }
      return
    }

    token.value = trimmedToken
    const result = await refreshState('verify')

    if (result === undefined) {
      return
    }

    activeTab.value = 'account'

    if (!options.silentSuccess) {
      notice.value = {
        tone: 'default',
        title: 'Bearer token accepted',
        description: 'The session controls are now available.',
      }
    }
  }

  function clearToken() {
    token.value = ''
    health.value = null
    sessionState.value = 'logged_out'
    activeTab.value = 'access'
    resetAuthorizationNotice()
  }

  async function submitCredentials(payload: CredentialsPayload) {
    const trimmedToken = token.value.trim()

    if (!trimmedToken) {
      clearToken()
      return
    }

    const result = await runAction('login', async () => {
      const response = await daemonRequest<AuthResponse>('/login', trimmedToken, {
        method: 'POST',
        body: JSON.stringify(payload),
      })

      sessionState.value = response.state
      return response
    })

    if (result === undefined) {
      return
    }

    if (result.status === 'need_2fa') {
      notice.value = {
        tone: 'default',
        title: 'Verification code required',
        description:
          result.message ?? 'Enter the Apple two-factor code to finish sign in.',
      }
      return
    }

    await refreshState('refresh')
    notice.value = {
      tone: 'default',
      title: 'Account connected',
      description: 'The Apple Music session is active on this daemon.',
    }
  }

  async function submitTwoFactor(code: string) {
    const trimmedToken = token.value.trim()

    if (!trimmedToken) {
      clearToken()
      return
    }

    const result = await runAction('two_factor', async () => {
      const response = await daemonRequest<AuthResponse>(
        '/login/2fa',
        trimmedToken,
        {
          method: 'POST',
          body: JSON.stringify({ code }),
        },
      )

      sessionState.value = response.state
      return response
    })

    if (result === undefined) {
      return
    }

    await refreshState('refresh')
    notice.value = {
      tone: 'default',
      title: 'Verification complete',
      description: 'The Apple Music session is ready.',
    }
  }

  async function resetPendingLogin() {
    const trimmedToken = token.value.trim()

    if (!trimmedToken) {
      clearToken()
      return
    }

    const result = await runAction('reset', async () =>
      daemonRequest<AuthResponse>('/login/reset', trimmedToken, {
        method: 'POST',
      }),
    )

    if (result === undefined) {
      return
    }

    await refreshState('refresh')
    notice.value = {
      tone: 'default',
      title: 'Pending login cleared',
      description: 'You can submit the Apple Music account again.',
    }
  }

  async function logout() {
    const trimmedToken = token.value.trim()

    if (!trimmedToken) {
      clearToken()
      return
    }

    const result = await runAction('logout', async () =>
      daemonRequest<AuthResponse>('/logout', trimmedToken, {
        method: 'POST',
      }),
    )

    if (result === undefined) {
      return
    }

    await refreshState('refresh')
    notice.value = {
      tone: 'default',
      title: 'Session cleared',
      description: 'The daemon is back in logged-out state.',
    }
  }

  async function initialize() {
    if (initialized.value) {
      return
    }

    initialized.value = true

    if (!token.value.trim()) {
      resetAuthorizationNotice()
      return
    }

    await verifyToken({ silentSuccess: true })
  }

  return {
    activeTab,
    actionInFlight,
    authorized,
    awaitingTwoFactor,
    clearToken,
    health,
    initialize,
    logout,
    notice,
    refreshState,
    resetPendingLogin,
    sessionState,
    submitCredentials,
    submitTwoFactor,
    token: tokenValue,
    verifyToken,
  }
}
