/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// A bundled desktop page runs on a Tauri-owned origin, so a "Copy link" that
// resolves its path against window.location.origin hands the user a URL
// nothing outside the app can open. These drive a real call site outside the
// work-item views with only the page origin and the baked web origin swapped.
const mocks = vi.hoisted(() => ({ toast: vi.fn() }));

vi.mock("@pi-dash/constants", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/constants")>()),
  WEB_BASE_URL: "https://pidash.airepublic.com",
}));
vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { SUCCESS: "success", ERROR: "error", INFO: "info" },
  setToast: mocks.toast,
}));

import { useProjectActions } from "../../core/components/navigation/use-project-actions";

const writeText = vi.fn();

beforeEach(() => {
  writeText.mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
});

afterEach(() => {
  vi.unstubAllGlobals();
  writeText.mockReset();
  mocks.toast.mockReset();
});

describe("desktop Copy link outside work items", () => {
  it.each([
    ["macOS and Linux", "tauri://localhost"],
    ["Windows", "http://tauri.localhost"],
  ])("copies the hosted URL for a cycle on %s (page origin %s)", async (_platform, origin) => {
    vi.stubGlobal("location", { ...window.location, origin, href: `${origin}/acme/projects/project-id/cycles` });

    const { result } = renderHook(() =>
      useProjectActions({
        workspaceSlug: "acme",
        projectId: "project-id",
        activeItem: { href: "/acme/projects/project-id/cycles/cycle-id" } as never,
      })
    );
    await act(() => result.current.handleCopyText());

    expect(writeText).toHaveBeenCalledWith("https://pidash.airepublic.com/acme/projects/project-id/cycles/cycle-id");
    expect(mocks.toast).toHaveBeenCalledWith(expect.objectContaining({ title: "Link copied!" }));
  });

  // copyUrlToClipboard() from @pi-dash/utils is the helper that reads the page
  // origin. Nothing in the desktop bundle may call it: share links go through
  // @/utils/share-url, which this overlay points at the hosted origin.
  it("leaves no call site in the bundle on the page-origin helper", () => {
    const webRoot = path.resolve(import.meta.dirname, "../..");
    const offenders = ["app", "ce", "core", "helpers"]
      .flatMap((dir) =>
        readdirSync(path.join(webRoot, dir), { recursive: true, encoding: "utf8" }).map((file) => path.join(dir, file))
      )
      .filter((file) => /\.tsx?$/.test(file))
      .filter((file) => /\bcopyUrlToClipboard\b/.test(readFileSync(path.join(webRoot, file), "utf8")));

    expect(offenders).toEqual([]);
  });
});
