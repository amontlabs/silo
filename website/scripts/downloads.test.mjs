import test from "node:test";
import assert from "node:assert/strict";
import { linuxDownloads } from "../src/downloads.js";

test("each Linux architecture selects both matching published package formats", () => {
  for (const architecture of ["x64", "arm64"]) {
    const { deb, appImage } = linuxDownloads(architecture);
    assert.equal(
      deb,
      `https://github.com/amontlabs/silo/releases/latest/download/Silo-linux-${architecture}.deb`,
    );
    assert.equal(
      appImage,
      `https://github.com/amontlabs/silo/releases/latest/download/Silo-linux-${architecture}.AppImage`,
    );
  }
});

test("an unsupported architecture cannot silently download the wrong binary", () => {
  for (const architecture of ["x86", "intel", "", "../macos", undefined]) {
    assert.throws(() => linuxDownloads(architecture), /Choose x64 or ARM64/);
  }
});
