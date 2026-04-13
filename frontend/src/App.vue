<script setup lang="ts">
import { onMounted } from 'vue'

import BearerPage from '@/components/auth/BearerPage.vue'
import SettingsPage from '@/components/auth/SettingsPage.vue'
import { useDaemonClient } from '@/composables/useDaemonClient'

const {
  actionInFlight,
  authorized,
  initialize,
  logout,
  notice,
  resetPendingLogin,
  sessionState,
  submitCredentials,
  submitTwoFactor,
  token,
  verifyToken,
} = useDaemonClient()

onMounted(() => {
  void initialize()
})
</script>

<template>
  <BearerPage
    v-if="!authorized"
    v-model:token="token"
    :busy="actionInFlight === 'verify'"
    :notice="notice"
    @submit="verifyToken"
  />

  <SettingsPage
    v-else
    :busy-action="actionInFlight"
    :notice="notice"
    :session-state="sessionState"
    @login="submitCredentials"
    @logout="logout"
    @reset-two-factor="resetPendingLogin"
    @submit-two-factor="submitTwoFactor"
  />
</template>
