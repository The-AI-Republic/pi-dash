/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The chat activity strip used to print `event.kind`, and the daemon forwards
 * every engine frame it does not classify as `kind: "raw"` — so a turn showed
 * a stack of identical "raw" rows with the real method hidden in the payload.
 *
 * The frames below are the ones a real turn produced (captured against the
 * bundled engine on 2026-09-16, including a shell tool call).
 */

import { describe, expect, it } from "vitest";
import { engineActivityLabel } from "@/app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page";
import type { IAgentChatEvent } from "@pi-dash/types";

function raw(method: string, params: Record<string, unknown> = {}): IAgentChatEvent {
  return {
    id: 1,
    session: "s",
    message: null,
    seq: 1,
    kind: "raw",
    payload: { method, params },
    created_at: "",
  } as IAgentChatEvent;
}

describe("engineActivityLabel", () => {
  it("names a shell tool call, running and finished", () => {
    const item = { type: "commandExecution", command: "/bin/bash -lc 'echo hi'" };
    expect(engineActivityLabel(raw("item/started", { item }))).toBe("Running: /bin/bash -lc 'echo hi'");
    expect(engineActivityLabel(raw("item/completed", { item }))).toBe("Ran: /bin/bash -lc 'echo hi'");
  });

  it("names a file edit", () => {
    const item = { type: "fileChange", path: "src/main.rs" };
    expect(engineActivityLabel(raw("item/started", { item }))).toBe("Editing src/main.rs");
  });

  it("surfaces engine warnings", () => {
    expect(engineActivityLabel(raw("warning", { message: "Code Mode is unavailable" }))).toBe(
      "Warning: Code Mode is unavailable"
    );
  });

  it("hides the chat messages themselves — the transcript renders those", () => {
    expect(engineActivityLabel(raw("item/started", { item: { type: "userMessage" } }))).toBeNull();
    expect(engineActivityLabel(raw("item/completed", { item: { type: "agentMessage" } }))).toBeNull();
  });

  it("hides startup chatter and accounting", () => {
    for (const method of [
      "remoteControl/status/changed",
      "thread/status/changed",
      "thread/tokenUsage/updated",
      "account/rateLimits/updated",
      "mcpServer/startupStatus/updated",
    ]) {
      expect(engineActivityLabel(raw(method)), method).toBeNull();
    }
  });

  it("falls back to the method name rather than the word raw", () => {
    expect(engineActivityLabel(raw("something/new"))).toBe("something/new");
  });
});
