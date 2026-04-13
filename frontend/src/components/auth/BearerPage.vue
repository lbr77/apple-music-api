<script setup lang="ts">
import { computed } from 'vue'

import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { type Notice } from '@/composables/useDaemonClient'

const props = defineProps<{
  busy: boolean
  notice: Notice
  token: string
}>()

const emit = defineEmits<{
  'update:token': [value: string]
  submit: []
}>()

const tokenValue = computed({
  get: () => props.token,
  set: (value: string | number) => {
    emit('update:token', String(value))
  },
})
</script>

<template>
  <section class="flex min-h-screen items-center justify-center bg-white px-4">
    <Card class="w-full max-w-sm border-neutral-200 shadow-none">
      <CardHeader class="text-center">
        <CardTitle class="text-xl font-medium">Bearer Login</CardTitle>
        <CardDescription>
          {{ notice.tone === 'destructive' ? notice.description : 'Enter the daemon Bearer token.' }}
        </CardDescription>
      </CardHeader>

      <CardContent>
        <Input
          v-model="tokenValue"
          type="password"
          placeholder="Bearer token"
          autocomplete="current-password"
        />
      </CardContent>

      <CardFooter>
        <Button class="w-full" :disabled="busy || token.trim().length === 0" @click="emit('submit')">
          {{ busy ? 'Logging in…' : 'Login' }}
        </Button>
      </CardFooter>
    </Card>
  </section>
</template>
