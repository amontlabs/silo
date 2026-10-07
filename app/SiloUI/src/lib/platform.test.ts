import { afterEach, expect, it, vi } from "vitest"

import { isLinux, isMac } from "@/lib/platform"

afterEach(() => vi.restoreAllMocks())

it.each([
  ["MacIntel", true, false],
  ["Linux x86_64", false, true],
  ["Win32", false, false],
])("classifies the platform %s", (name, mac, linux) => {
  vi.spyOn(navigator, "platform", "get").mockReturnValue(name)
  expect(isMac()).toBe(mac)
  expect(isLinux()).toBe(linux)
})
