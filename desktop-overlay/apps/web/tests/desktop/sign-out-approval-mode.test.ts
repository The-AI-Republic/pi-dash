/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "http://localhost:18002" }));

/** The key the chat page reads and writes for the built-in agent. */
const KEY = "pidash:chat-approval-mode:pidash-builtin";

/**
 * The host's per-account settings. Unlike `localStorage` it is a file on disk,
 * so it outlives both the webview wipe and the page reload that follows.
 */
const disk = new Map<string, Record<string, string>>();

let saveFails = false;

const invoke = vi.fn(async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
  const account = String(args?.account ?? "");
  switch (command) {
    // `clear_all_browsing_data` drops the whole profile, storage included.
    case "desktop_clear_web_data":
      localStorage.clear();
      sessionStorage.clear();
      return undefined;
    case "chat_approval_modes_save":
      if (saveFails) throw new Error("disk full");
      disk.set(account, { ...disk.get(account), ...(args?.modes as Record<string, string>) });
      return undefined;
    case "chat_approval_modes_load":
      return disk.get(account) ?? {};
    default:
      return {};
  }
});

/** The chat page's own resolution of the stored value (`runners/chat/[runnerId]/page.tsx`). */
function modeTheChatPageResolves(): string {
  const stored = localStorage.getItem(KEY);
  return stored === "ask" || stored === "workspace" || stored === "full_access" ? stored : "full_access";
}

/** A fresh page load with `userId` signed in — what the root `AgentRuntime` effect does. */
async function bootSignedIn(userId: string) {
  vi.resetModules();
  const { resumeAgentRuntime } = await import("@/services/agent-runtime");
  resumeAgentRuntime(userId);
  // Let the host round trip that follows sign-in settle.
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function signOut() {
  const { performSignOut } = await import("@/services/auth-signout");
  await performSignOut({ requestCSRFToken: async () => ({ csrf_token: "csrf" }) }, "http://localhost:18002");
}

beforeEach(() => {
  disk.clear();
  saveFails = false;
  invoke.mockClear();
  localStorage.clear();
  sessionStorage.clear();
  document.body.innerHTML = "";
  Object.assign(window, { __TAURI__: { core: { invoke } } });
  // The sign-out form POST is a real navigation; there is nowhere to go here.
  vi.spyOn(HTMLFormElement.prototype, "submit").mockImplementation(() => {});
});

describe("approval mode across a desktop sign-out", () => {
  it("is still the saved mode after signing out and back in", async () => {
    await bootSignedIn("user-a");
    localStorage.setItem(KEY, "ask");

    await signOut();
    // The wipe really happened: nothing is left in the webview.
    expect(localStorage.getItem(KEY)).toBeNull();

    await bootSignedIn("user-a");
    expect(modeTheChatPageResolves()).toBe("ask");
  });

  it("does not hand one account's mode to the next account on the machine", async () => {
    await bootSignedIn("user-a");
    localStorage.setItem(KEY, "ask");
    await signOut();

    await bootSignedIn("user-b");
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it("leaves a mode that is already in the webview alone", async () => {
    disk.set("user-a", { "pidash-builtin": "ask" });
    localStorage.setItem(KEY, "workspace");

    await bootSignedIn("user-a");
    expect(localStorage.getItem(KEY)).toBe("workspace");
  });

  it("still wipes the webview when the mode cannot be saved", async () => {
    await bootSignedIn("user-a");
    localStorage.setItem(KEY, "ask");
    saveFails = true;

    await signOut();
    expect(invoke).toHaveBeenCalledWith("desktop_clear_web_data");
    expect(localStorage.getItem(KEY)).toBeNull();
  });
});
