<script setup lang="ts">
import { reactive, shallowRef, watch } from 'vue'

import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { type ActionName, type Notice } from '@/composables/useDaemonClient'
import { type SessionState } from '@/lib/api'

interface CredentialsPayload {
  username: string
  password: string
}

const props = defineProps<{
  busyAction: ActionName | null
  notice: Notice
  sessionState: SessionState
}>()

const emit = defineEmits<{
  login: [payload: CredentialsPayload]
  logout: []
  resetTwoFactor: []
  submitTwoFactor: [code: string]
}>()

const credentials = reactive({
  username: '',
  password: '',
})
const verificationCode = shallowRef('')

watch(
  () => props.sessionState,
  (state) => {
    if (state !== 'awaiting_2fa') {
      verificationCode.value = ''
    }
  },
)

function submitCredentials() {
  emit('login', {
    username: credentials.username.trim(),
    password: credentials.password,
  })
  credentials.password = ''
}

function submitTwoFactor() {
  emit('submitTwoFactor', verificationCode.value.trim())
}

function handleDialogState(open: boolean) {
  if (!open && props.sessionState === 'awaiting_2fa') {
    emit('resetTwoFactor')
  }
}
</script>

<template>
  <section class="flex min-h-screen items-center justify-center bg-background px-4 py-10">
    <Card class="w-full max-w-[420px] border-border">
      <CardHeader class="text-center">
        <CardTitle class="text-[1.375rem] font-semibold">Settings</CardTitle>
        <CardDescription>
          {{ notice.tone === 'destructive' ? notice.description : sessionState === 'logged_in' ? 'Current state is logged in.' : 'Current state is logged out.' }}
        </CardDescription>
      </CardHeader>

      <CardContent v-if="sessionState === 'logged_out'" class="space-y-4">
        <div class="space-y-2">
          <Label for="apple-username">Username</Label>
          <Input
            id="apple-username"
            v-model="credentials.username"
            autocomplete="username"
            placeholder="Apple ID"
            :disabled="busyAction !== null"
          />
        </div>

        <div class="space-y-2">
          <Label for="apple-password">Password</Label>
          <Input
            id="apple-password"
            v-model="credentials.password"
            type="password"
            autocomplete="current-password"
            placeholder="Password"
            :disabled="busyAction !== null"
          />
        </div>
      </CardContent>

      <CardFooter v-if="sessionState === 'logged_out'" class="pt-1">
        <Button
          class="w-full"
          :disabled="
            busyAction !== null ||
            credentials.username.trim().length === 0 ||
            credentials.password.length === 0
          "
          @click="submitCredentials"
        >
          {{ busyAction === 'login' ? 'Submitting…' : 'Login' }}
        </Button>
      </CardFooter>

      <CardFooter v-else-if="sessionState === 'logged_in'" class="pt-1">
        <Button
          class="w-full"
          variant="outline"
          :disabled="busyAction !== null"
          @click="emit('logout')"
        >
          {{ busyAction === 'logout' ? 'Logging out…' : 'Logout' }}
        </Button>
      </CardFooter>
    </Card>

    <Dialog
      :open="sessionState === 'awaiting_2fa'"
      @update:open="handleDialogState"
    >
      <DialogContent class="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>Two-Factor</DialogTitle>
          <DialogDescription>
            Enter the Apple verification code.
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-2">
          <Label for="two-factor-code">Code</Label>
          <Input
            id="two-factor-code"
            v-model="verificationCode"
            autocomplete="one-time-code"
            inputmode="numeric"
            placeholder="123456"
            :disabled="busyAction !== null"
          />
        </div>

        <DialogFooter class="grid grid-cols-2 gap-2 sm:grid-cols-2">
          <Button
            variant="outline"
            :disabled="busyAction !== null"
            @click="emit('resetTwoFactor')"
          >
            Cancel
          </Button>
          <Button
            :disabled="busyAction !== null || verificationCode.trim().length === 0"
            @click="submitTwoFactor"
          >
            {{ busyAction === 'two_factor' ? 'Submitting…' : 'Confirm' }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </section>
</template>
