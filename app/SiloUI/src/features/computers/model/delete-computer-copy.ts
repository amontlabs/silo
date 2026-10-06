/** The title of every Delete computer confirmation (decision 6). */
export function deleteComputerTitle(displayName: string): string {
  return `Delete ${displayName} permanently?`
}

/** The consequence line shown by every Delete computer confirmation (row, row menu and page).
 * Deleting a computer removes its workspace disk and checkpoint history. */
export function deleteComputerDescription(checkpoints?: number, size?: string): string {
  const history = checkpoints === undefined ? "checkpoints" : checkpoints === 1 ? "1 checkpoint" : `${checkpoints} checkpoints`
  return `Its files${size ? ` (${size})` : ""} and ${history} will be deleted. This can't be undone.`
}
