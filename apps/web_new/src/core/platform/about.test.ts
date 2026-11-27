// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop About binding (DESK-028): the desktop platform reads the app
// version and the bundled licence texts from the shell's `desktop_about`
// command; the web platform has no such capability.
import { afterEach, describe, expect, it, vi } from "vitest";

import { platform, toAboutInfo } from "./tauri.js";
import { platform as webPlatform } from "./web.js";

const NATIVE_ORIGIN = "http://localhost:8000";

const NATIVE_ABOUT = {
  version: "1.4.0",
  notices: [
    { file: "pidash-LICENSE.txt", component: "Pi Dash", license: "AGPL-3.0-only", text: "GNU AFFERO" },
    { file: "agent-engine-NOTICE.txt", component: "Agent engine notice", license: "Apache-2.0", text: null },
  ],
};

function installBridge(invoke: (command: string, args?: unknown) => Promise<unknown>): ReturnType<typeof vi.fn> {
  const spy = vi.fn(invoke);
  const win = globalThis.window as unknown as Record<string, unknown>;
  win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
  win["__TAURI__"] = { core: { invoke: spy } };
  return spy;
}

afterEach(() => {
  const win = globalThis.window as unknown as Record<string, unknown>;
  delete win["__PIDASH_NATIVE_HTTP__"];
  delete win["__TAURI__"];
});

describe("desktop about", () => {
  it("reads the version and every bundled notice through desktop_about", async () => {
    const invoke = installBridge(async () => NATIVE_ABOUT);
    await expect(platform.about?.load()).resolves.toEqual(NATIVE_ABOUT);
    expect(invoke).toHaveBeenCalledWith("desktop_about", undefined);
  });

  it("rejects without the native bridge", async () => {
    await expect(platform.about?.load()).rejects.toThrow("desktop_about needs the native bridge");
  });

  it("passes the shell's refusal through", async () => {
    installBridge(async () => {
      throw new Error("resource dir: missing");
    });
    await expect(platform.about?.load()).rejects.toThrow("resource dir: missing");
  });

  it("is absent on the web platform", () => {
    expect(webPlatform.about).toBeUndefined();
  });
});

describe("about payload", () => {
  it("keeps a notice whose text this build did not stage", () => {
    expect(toAboutInfo(NATIVE_ABOUT).notices[1]).toEqual({
      file: "agent-engine-NOTICE.txt",
      component: "Agent engine notice",
      license: "Apache-2.0",
      text: null,
    });
  });

  it("drops notices without a file name and blanks a missing version", () => {
    expect(toAboutInfo({ notices: [{ component: "x" }, "junk"] })).toEqual({ version: "", notices: [] });
    expect(toAboutInfo(null)).toEqual({ version: "", notices: [] });
  });

  it("reads a non-string text as not staged", () => {
    const info = toAboutInfo({ version: "1", notices: [{ file: "a.txt", text: 3 }] });
    expect(info.notices).toEqual([{ file: "a.txt", component: "a.txt", license: "", text: null }]);
  });
});
