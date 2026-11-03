// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clipboardPermissionGranted,
  clipboardWriteAvailable,
  readClipboardWriteAllowed,
  useClipboardWritePermission,
  type ClipboardCapable,
} from "./clipboard.js";

function capable(overrides: Partial<ClipboardCapable> = {}): ClipboardCapable {
  return { clipboard: { writeText: () => Promise.resolve() }, ...overrides };
}

/** jsdom's navigator carries neither property, so each test installs one. */
function stubNavigatorProperty(name: "clipboard" | "permissions", value: unknown): void {
  Object.defineProperty(globalThis.navigator, name, { value, configurable: true });
}

afterEach(() => {
  vi.restoreAllMocks();
  for (const name of ["clipboard", "permissions"] as const) {
    Reflect.deleteProperty(globalThis.navigator, name);
  }
});

describe("clipboardWriteAvailable", () => {
  it("requires a writeText function on the clipboard", () => {
    expect(clipboardWriteAvailable(capable())).toBe(true);
    expect(clipboardWriteAvailable({ clipboard: {} })).toBe(false);
    expect(clipboardWriteAvailable({ clipboard: { writeText: undefined } })).toBe(false);
    expect(clipboardWriteAvailable(undefined)).toBe(false);
  });
});

describe("clipboardPermissionGranted", () => {
  it("requires an explicit grant (a prompt answers no copy action)", () => {
    expect(clipboardPermissionGranted({ state: "granted" })).toBe(true);
    expect(clipboardPermissionGranted({ state: "denied" })).toBe(false);
    expect(clipboardPermissionGranted({ state: "prompt" })).toBe(false);
  });
});

describe("readClipboardWriteAllowed", () => {
  it("is false without clipboard writing", async () => {
    await expect(readClipboardWriteAllowed({})).resolves.toBe(false);
  });

  it("is true when the permissions API cannot be asked", async () => {
    await expect(readClipboardWriteAllowed(capable())).resolves.toBe(true);
  });

  it("follows the permission probe and treats a query failure as allowed", async () => {
    await expect(
      readClipboardWriteAllowed(capable({ permissions: { query: () => Promise.resolve({ state: "granted" }) } }))
    ).resolves.toBe(true);
    await expect(
      readClipboardWriteAllowed(capable({ permissions: { query: () => Promise.resolve({ state: "denied" }) } }))
    ).resolves.toBe(false);
    await expect(
      readClipboardWriteAllowed(capable({ permissions: { query: () => Promise.resolve({ state: "prompt" }) } }))
    ).resolves.toBe(false);
    await expect(
      readClipboardWriteAllowed(capable({ permissions: { query: () => Promise.reject(new Error("unsupported")) } }))
    ).resolves.toBe(true);
  });
});

describe("useClipboardWritePermission", () => {
  it("starts closed and settles open once the probe resolves", async () => {
    stubNavigatorProperty("clipboard", { writeText: () => Promise.resolve() });
    const { result } = renderHook(() => useClipboardWritePermission());
    expect(result.current).toBe(false);
    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });

  it("stays closed when the probe answers after unmount", async () => {
    let release: (value: { state: string }) => void = () => undefined;
    stubNavigatorProperty("permissions", { query: () => new Promise((resolve) => (release = resolve)) });
    const { result, unmount } = renderHook(() => useClipboardWritePermission());
    unmount();
    await act(async () => {
      release({ state: "granted" });
      await Promise.resolve();
    });
    expect(result.current).toBe(false);
  });
});
