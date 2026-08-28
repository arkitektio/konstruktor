import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/utils"

/**
 * A line of explanation attached to what is above it.
 *
 * Flex, not grid. The shadcn original lays out `[0_1fr]` columns and expects every child
 * to be an `AlertTitle` or an `AlertDescription`, which are placed in column two by hand;
 * every caller in this app passes a bare string or an element instead, and those become
 * anonymous grid items in the zero-width first column — the alerts rendered as a narrow
 * ribbon of wrapped text with the rest of the box empty. A flex row grows its children,
 * so plain children work, which is how they are all written.
 *
 * The default variant is deliberately quiet: most of these say a thing is *fine* ("this
 * folder can be used"), and a card-coloured box with a full-strength border read like a
 * warning about the very thing it was reassuring you about.
 */
const alertVariants = cva(
  "relative flex w-full items-start gap-3 rounded-lg border px-4 py-3 text-sm [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:translate-y-0.5 [&>svg]:text-current",
  {
    variants: {
      variant: {
        default: "border-border/50 bg-muted/30 text-muted-foreground",
        destructive:
          "border-destructive/40 bg-destructive/5 text-destructive *:data-[slot=alert-description]:text-destructive/90 [&>svg]:text-current",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  }
)

function Alert({
  className,
  variant,
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof alertVariants>) {
  return (
    <div
      data-slot="alert"
      role="alert"
      className={cn(alertVariants({ variant }), className)}
      {...props}
    />
  )
}

/**
 * A heading inside an alert. With the flex layout, a title and a description are stacked
 * by wrapping them in a plain `div` — they are siblings of the icon otherwise.
 */
function AlertTitle({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="alert-title"
      className={cn("line-clamp-1 min-h-4 font-medium tracking-tight", className)}
      {...props}
    />
  )
}

function AlertDescription({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="alert-description"
      className={cn(
        "grid justify-items-start gap-1 text-sm text-muted-foreground [&_p]:leading-relaxed",
        className
      )}
      {...props}
    />
  )
}

export { Alert, AlertTitle, AlertDescription }
