import { Spinner } from "@/components/ui/spinner"
import { statusTones } from "@/components/status-tone"
import { cn } from "@/lib/utils"
import type { StatusPresentation } from "@/features/onboarding/components/status-presentation"

/** A small tone-colored status label, with a spinner while the work is in progress. */
export function StatusLabel({ tone, label, busy = false }: StatusPresentation & { busy?: boolean }) {
  return <span className={cn("inline-flex shrink-0 items-center gap-1 text-caption font-normal", statusTones[tone].text)}>{busy && <Spinner size="sm" />}{label}</span>
}
