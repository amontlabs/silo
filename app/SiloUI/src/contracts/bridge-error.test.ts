import { readFileSync } from "node:fs"
import { describe, expect, it } from "vitest"
import { bridgeErrorCodes, bridgeErrorMessage, bridgeErrorSchema, hasBridgeErrorCode } from "./bridge-error"
import { isUnsupportedRemote } from "@/features/application/model/logs"
import { isCancelledError } from "@/features/application/model/operation-queue"
import { isUpdateInProgress } from "@/desktop/production-source"
import fixture from "@/test/contracts/bridge-errors.json"

describe("native bridge errors", () => {
  it("accepts Rust-emitted errors and has exactly the Rust enum's codes", () => {
    const rust = readFileSync(new URL("../../src-tauri/engine/src/bridge_error.rs", import.meta.url), "utf8")
    const variants = /pub enum ErrorCode\s*\{([^}]+)\}/.exec(rust)![1]
      .split(",").map(value => value.trim()).filter(Boolean)
      .map(value => value.replace(/[A-Z]/g, (letter, index) => `${index ? "_" : ""}${letter.toLowerCase()}`))
    expect([...bridgeErrorCodes].sort()).toEqual(variants.sort())
    expect(fixture.map(error => bridgeErrorSchema.parse(error).code).sort()).toEqual([...bridgeErrorCodes].sort())
  })

  it("classifies recovery by code even when all display text changes", () => {
    expect(isUpdateInProgress({ code: "update_in_progress", message: "Please wait." })).toBe(true)
    expect(isUnsupportedRemote({ code: "unsupported_remote_operation", message: "Update the owner." })).toBe(true)
    expect(isCancelledError({ code: "cancelled", message: "Stopped at your request." })).toBe(true)
    for (const cause of ["SILO_SANDBOX_UPDATE_IN_PROGRESS", new Error("Unsupported remote request."), "The operation was cancelled.", { code: "internal", message: "The operation was cancelled." }]) {
      expect(isUpdateInProgress(cause)).toBe(false)
      expect(isUnsupportedRemote(cause)).toBe(false)
      expect(isCancelledError(cause)).toBe(false)
    }
  })

  it("displays unknown error codes without applying known recovery policies", () => {
    const error = { code: "future_error", message: "Update the owner." }
    expect(bridgeErrorMessage(error)).toBe("Update the owner.")
    expect(hasBridgeErrorCode(error, "cancelled")).toBe(false)
    expect(bridgeErrorMessage({ code: "cancelled", message: null })).toBeUndefined()
    expect(isCancelledError({ code: "cancelled" })).toBe(false)
  })
})
