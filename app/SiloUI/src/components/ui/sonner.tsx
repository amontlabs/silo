import { Toaster as Sonner, type ToasterProps } from "sonner"
import { CircleCheckIcon, InfoIcon, TriangleAlertIcon, OctagonXIcon } from "lucide-react"

import { useTheme } from "@/features/preferences/theme"
import { Spinner } from "@/components/ui/spinner"

// The shadcn template pulls the theme from next-themes. Silo is not a Next app, so the
// theme comes from the app's own settings store (see features/preferences/theme.ts), which
// also drives the `.dark` class on <html> that our CSS variables key off of.
const Toaster = ({ reduceMotion = false, ...props }: ToasterProps & { reduceMotion?: boolean }) => {
  const { theme } = useTheme()

  return (
    <Sonner
      theme={theme as ToasterProps["theme"]}
      className="toaster group silo-toaster"
      data-reduce-motion={reduceMotion || undefined}
      position="bottom-right"
      icons={{
        success: (
          <CircleCheckIcon className="size-4" />
        ),
        info: (
          <InfoIcon className="size-4" />
        ),
        warning: (
          <TriangleAlertIcon className="size-4" />
        ),
        error: (
          <OctagonXIcon className="size-4" />
        ),
        loading: (
          <Spinner className="size-4" />
        ),
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as React.CSSProperties
      }
      // Persistent and actionable notifications must not be silently collapsed behind newer
      // ones. The close button is restyled in index.css (.silo-toaster).
      visibleToasts={5}
      expand
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
