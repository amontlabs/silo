const trimmed = (value: number, digits: number) => Number(value.toFixed(digits)).toString()

/** Binary units, as the host allocation figures are measured; whole values drop their decimals. */
export function formatBinaryBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 ** 2) return `${trimmed(bytes / 1024, 1)} KiB`
  return bytes >= 1024 ** 3 ? `${trimmed(bytes / 1024 ** 3, 2)} GiB` : `${trimmed(bytes / 1024 ** 2, 1)} MiB`
}

/** Decimal units, as file managers show the size of a file being transferred. */
export function formatDecimalBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} bytes`
  const units = ["KB", "MB", "GB"]
  let value = bytes
  let unit = -1
  while (value >= 1000 && unit < units.length - 1) { value /= 1000; unit++ }
  return `${value.toFixed(1)} ${units[unit]}`
}
