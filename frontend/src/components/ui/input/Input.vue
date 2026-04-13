<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { useVModel } from "@vueuse/core"
import { cn } from "@/lib/utils"

const props = defineProps<{
  defaultValue?: string | number
  modelValue?: string | number
  class?: HTMLAttributes["class"]
}>()

const emits = defineEmits<{
  (e: "update:modelValue", payload: string | number): void
}>()

const modelValue = useVModel(props, "modelValue", emits, {
  passive: true,
  defaultValue: props.defaultValue,
})
</script>

<template>
  <input
    v-model="modelValue"
    data-slot="input"
    :class="cn(
      'file:text-foreground placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground border-input h-[44px] w-full min-w-0 rounded-lg border bg-white px-4 py-[11px] text-sm text-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.72)] transition-[border-color,box-shadow,background-color] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:bg-secondary disabled:opacity-60',
      'focus-visible:border-foreground/15 focus-visible:ring-[3px] focus-visible:ring-ring',
      'aria-invalid:border-destructive aria-invalid:ring-destructive/20',
      props.class,
    )"
  >
</template>
