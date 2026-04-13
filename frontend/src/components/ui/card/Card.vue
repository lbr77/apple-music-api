<script setup lang="ts">
import type { HTMLAttributes } from "vue"
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: HTMLAttributes["class"]
}>()
</script>

<template>
  <div
    data-slot="card"
    :class="
      cn(
        'bg-card text-card-foreground flex flex-col gap-6 rounded-[28px] border py-7 shadow-[0_24px_64px_rgba(0,0,0,0.08)] backdrop-blur-xl',
        props.class,
      )
    "
  >
    <slot />
  </div>
</template>
