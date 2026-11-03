// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const gate = vi.hoisted(() => {
  let release: () => void = () => undefined;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release: () => release() };
});

const prompts = vi.hoisted(() => ({
  ask: 0,
  answer: { deleteChatHistory: true } as { deleteChatHistory: boolean } | null,
}));

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  return { ...original, platform: { ...original.platform, kind: "desktop", agentRuntime: {} } };
});

vi.mock("./SignOutDialog.js", async () => {
  await gate.promise;
  const { registerSignOutPrompt } = await import("./signOutPrompt.js");
  function RuntimeSignOutDialog(): React.ReactElement {
    React.useEffect(() => {
      registerSignOutPrompt((resolve) => {
        prompts.ask += 1;
        resolve(prompts.answer);
      });
      return () => registerSignOutPrompt(null);
    }, []);
    return <p>slow sign-out dialog</p>;
  }
  return { RuntimeSignOutDialog };
});

import { getRuntimeSignOutHooks, registerRuntimeSignOutHooks } from "../../../core/session/index.js";
import { RuntimeSignOutLifecycle } from "./SignOutLifecycle.js";

afterEach(() => {
  registerRuntimeSignOutHooks(null);
  prompts.ask = 0;
});

describe("RuntimeSignOutLifecycle while the dialog chunk is loading", () => {
  it("holds a sign-out until the dialog can ask, and lets it cancel that sign-out", async () => {
    render(<RuntimeSignOutLifecycle />);
    let settled = false;
    const pending = getRuntimeSignOutHooks()!
      .confirmRuntimeSignOut()
      .then((choice) => {
        settled = true;
        return choice;
      });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(settled).toBe(false);
    expect(prompts.ask).toBe(0);
    gate.release();
    expect(await screen.findByText("slow sign-out dialog")).toBeVisible();
    await expect(pending).resolves.toEqual({ deleteChatHistory: true });
    expect(prompts.ask).toBe(1);

    prompts.answer = null;
    await expect(getRuntimeSignOutHooks()!.confirmRuntimeSignOut()).resolves.toBeNull();
    expect(prompts.ask).toBe(2);
  });
});
