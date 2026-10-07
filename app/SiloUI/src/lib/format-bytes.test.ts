import { expect, it } from "vitest"

import { formatBinaryBytes, formatDecimalBytes } from "@/lib/format-bytes"

it.each([
  [0, "0 B"],
  [1, "1 B"],
  [1023, "1023 B"],
  [1024, "1 KiB"],
  [4096, "4 KiB"],
  [1536, "1.5 KiB"],
  [512 * 1024 ** 2, "512 MiB"],
  [4.25 * 1024 ** 3, "4.25 GiB"],
  [1024 ** 2, "1 MiB"],
  [1024 ** 3, "1 GiB"],
  [1024 ** 4, "1024 GiB"],
])("formats %i bytes in binary units as %s", (bytes, expected) => {
  expect(formatBinaryBytes(bytes)).toBe(expected)
})

it.each([
  [0, "0 bytes"],
  [999, "999 bytes"],
  [1000, "1.0 KB"],
  [1_500_000, "1.5 MB"],
  [2_000_000_000, "2.0 GB"],
  [5_000_000_000_000, "5000.0 GB"],
])("formats %i bytes in decimal units as %s", (bytes, expected) => {
  expect(formatDecimalBytes(bytes)).toBe(expected)
})
