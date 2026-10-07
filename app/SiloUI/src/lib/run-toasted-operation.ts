import { errorMessage } from "@/lib/error-message"
import { showOperationFailure, showOperationProgress, showOperationSuccess, type OperationProgressOptions } from "@/lib/operation-toast"

type SuccessOptions = NonNullable<Parameters<typeof showOperationSuccess>[2]>
type FailureOptions = NonNullable<Parameters<typeof showOperationFailure>[2]>

interface ToastedOperation<T> {
  /** The notification id shared by the progress, success and failure phases. */
  id: string
  /** Shown while `work` runs. Omit when `work` shows its own progress. */
  progress?: OperationProgressOptions
  work: () => Promise<T>
  /** Shown when `work` resolves. Omit when `work` reports its own outcome. */
  success?: { title: string } & SuccessOptions
  /** Shown when `work` rejects, with the error as its description. */
  failure: { title: string; fallback?: string } & Omit<FailureOptions, "description">
  /** Runs after the success notification; an error it throws is reported as the failure. */
  onSuccess?: (value: T) => void
}

/**
 * The shared notification lifecycle for a background action: progress, then success or a failure
 * with Retry (see `operation-toast.ts`). Resolves to whether `work` succeeded.
 */
export async function runToastedOperation<T>({ id, progress, work, success, failure, onSuccess }: ToastedOperation<T>): Promise<boolean> {
  if (progress) showOperationProgress(id, progress)
  try {
    const value = await work()
    if (success) {
      const { title, ...options } = success
      showOperationSuccess(id, title, options)
    }
    onSuccess?.(value)
    return true
  } catch (cause) {
    const { title, fallback, ...options } = failure
    showOperationFailure(id, title, { ...options, description: errorMessage(cause, fallback === undefined ? undefined : { fallback }) })
    return false
  }
}
