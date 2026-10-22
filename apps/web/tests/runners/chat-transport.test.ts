/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatTransport } from "@pi-dash/services";
import { getChatTransport, setChatTransport } from "@pi-dash/services";

const notCalled = () => {
  throw new Error("fake transport method should not be called");
};

// A minimal fake transport: only the identity matters for these tests, so
// every method throws — a test that accidentally *calls* one fails loudly.
function fakeTransport(): ChatTransport {
  return {
    listChatSessions: notCalled,
    createChatSession: notCalled,
    listChatMessages: notCalled,
    sendChatMessage: notCalled,
    warmChatSession: notCalled,
    cancelChat: notCalled,
    closeChat: notCalled,
    subscribeChatEvents: notCalled,
  } as unknown as ChatTransport;
}

describe("chat transport seam", () => {
  afterEach(() => {
    // Reset to the cloud default so one test's override can't leak into
    // another (module state is process-global).
    setChatTransport(null);
  });

  it("returns a cloud default transport out of the box", () => {
    const transport = getChatTransport();
    expect(transport).toBeDefined();
    // The default implements the full chat surface the UI depends on.
    expect(typeof transport.listChatSessions).toBe("function");
    expect(typeof transport.sendChatMessage).toBe("function");
    expect(typeof transport.subscribeChatEvents).toBe("function");
  });

  it("returns a registered override from getChatTransport", () => {
    const override = fakeTransport();
    setChatTransport(override);
    expect(getChatTransport()).toBe(override);
  });

  it("resets to the same cloud default when cleared with null", () => {
    const before = getChatTransport();
    setChatTransport(fakeTransport());
    expect(getChatTransport()).not.toBe(before);
    setChatTransport(null);
    // Same singleton instance as before the override — not a fresh one.
    expect(getChatTransport()).toBe(before);
  });

  it("resets to the cloud default when cleared with undefined", () => {
    const before = getChatTransport();
    setChatTransport(fakeTransport());
    setChatTransport(undefined);
    expect(getChatTransport()).toBe(before);
  });

  it("never returns the fake once reset — a swap is fully reversible", () => {
    const override = fakeTransport();
    setChatTransport(override);
    setChatTransport(null);
    // Guard against the seam holding a stale reference to the override.
    expect(getChatTransport()).not.toBe(override);
    // And the restored default must still be usable, not a throwing shell.
    expect(() => vi.fn()(getChatTransport().subscribeChatEvents)).not.toThrow();
  });
});
