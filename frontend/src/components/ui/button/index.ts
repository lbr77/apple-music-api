import type { VariantProps } from "class-variance-authority"
import { cva } from "class-variance-authority"

export { default as Button } from "./Button.vue"

export const buttonVariants = cva(
  "inline-flex cursor-pointer items-center justify-center gap-2 whitespace-nowrap rounded-lg border border-transparent text-sm font-medium leading-none no-underline transition-[opacity,background-color,border-color,color,box-shadow] duration-150 disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 [&_svg]:shrink-0 outline-none focus-visible:ring-[3px] focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background aria-invalid:border-destructive aria-invalid:ring-destructive/20",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground shadow-[0_1px_2px_rgba(0,0,0,0.04)] hover:opacity-80",
        destructive:
          "bg-destructive text-white shadow-[0_1px_2px_rgba(220,38,38,0.18)] hover:opacity-80 focus-visible:ring-destructive/20",
        outline:
          "border-border bg-secondary text-secondary-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.7)] hover:opacity-80",
        secondary:
          "border-border bg-white text-foreground shadow-[0_1px_2px_rgba(0,0,0,0.03)] hover:opacity-80",
        ghost:
          "text-muted-foreground hover:bg-secondary hover:text-foreground",
        link: "border-0 px-0 py-0 text-foreground underline-offset-4 hover:opacity-70 hover:underline",
      },
      size: {
        "default": "min-h-[44px] px-[22px] py-[11px]",
        "sm": "min-h-9 gap-1.5 px-4 py-2 text-[13px]",
        "lg": "min-h-12 px-6 py-3 text-[15px]",
        "icon": "size-[44px] p-0",
        "icon-sm": "size-9 p-0",
        "icon-lg": "size-12 p-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
)
export type ButtonVariants = VariantProps<typeof buttonVariants>
