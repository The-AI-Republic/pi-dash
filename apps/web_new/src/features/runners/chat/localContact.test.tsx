// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Built-in contact availability (NEWFRONT-553, RUN-033): the contact is
// absent without the local platform capability, linked when the server
// reports the agent available, and shown with the server's reason when it
// refuses. A failed check reads as unavailable in its own words.
import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ getAgentProfile: vi.fn() }));
const platformState = vi.hoisted(() => ({ localChat: true }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return {
      kind: "desktop",
      get localChat() {
        return platformState.localChat ? {} : undefined;
      },
    };
  },
}));

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { testClient } = await import("./harness.js");
const { LOCAL_CONTACT_LABEL, LOCAL_CONTACT_NAME, hasLocalChat, localContactAvailability, useLocalContact } =
  await import("./localContact.js");

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: ReturnType<typeof testClient>;
  children?: React.ReactNode;
}>;

function setup() {
  const client = testClient();
  const wrapper = ({ children }: { children: React.ReactNode }) => <Provider client={client}>{children}</Provider>;
  return renderHook(() => useLocalContact(), { wrapper });
}

function profile(available: boolean, reason_code = ""): { available: boolean; reason_code: string } {
  return { available, reason_code };
}

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
  contracts.getAgentProfile.mockResolvedValue(profile(true));
  platformState.localChat = false;
});

afterEach(() => {
  contracts.getAgentProfile.mockReset();
  resetAppClient();
});

describe("localContactAvailability", () => {
  it("translates an available answer and a refusal", () => {
    expect(localContactAvailability(profile(true))).toEqual({ unavailableReason: null });
    expect(localContactAvailability(profile(false, "managed_runner_disabled"))).toEqual({
      unavailableReason: "The built-in agent is switched off on this server.",
    });
    expect(localContactAvailability(profile(false, "odd_code"))).toEqual({ unavailableReason: "odd_code" });
  });
});

describe("hasLocalChat", () => {
  it("follows the platform's capability", () => {
    expect(hasLocalChat()).toBe(false);
    platformState.localChat = true;
    expect(hasLocalChat()).toBe(true);
  });
});

describe("useLocalContact", () => {
  it("offers no contact and reads nothing without the capability", () => {
    const { result } = setup();
    expect(result.current).toEqual([]);
    expect(contracts.getAgentProfile).not.toHaveBeenCalled();
  });

  it("offers a linked contact when the agent is available", async () => {
    platformState.localChat = true;
    contracts.getAgentProfile.mockResolvedValue(profile(true));
    const { result } = setup();
    await waitFor(() => expect(result.current[0]?.unavailableReason).toBeNull());
    expect(result.current).toEqual([
      { id: "pidash-builtin", name: LOCAL_CONTACT_NAME, label: LOCAL_CONTACT_LABEL, unavailableReason: null },
    ]);
  });

  it("offers the contact disabled with the server's reason while the check is out", async () => {
    platformState.localChat = true;
    let answer: (value: { available: boolean; reason_code: string }) => void = () => undefined;
    contracts.getAgentProfile.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    const { result } = setup();
    // Before the answer lands there is no reason: the entry reads as available
    // until the server says otherwise.
    expect(result.current[0]?.unavailableReason).toBeNull();
    answer(profile(false, "llm_config_missing"));
    await waitFor(() =>
      expect(result.current[0]?.unavailableReason).toBe(
        "No model is configured for AI runs yet. Set one up in the AI settings."
      )
    );
  });

  it("reads a failed check as unavailable in the failure's own words", async () => {
    platformState.localChat = true;
    contracts.getAgentProfile.mockRejectedValue(new Error("Desktop session required"));
    const { result } = setup();
    await waitFor(() => expect(result.current[0]?.unavailableReason).toBe("Desktop session required"));
  });

  it("reads an unreadable failure as a plain unavailability", async () => {
    platformState.localChat = true;
    contracts.getAgentProfile.mockRejectedValue({ code: 500 });
    const { result } = setup();
    await waitFor(() => expect(result.current[0]?.unavailableReason).toBe("The built-in agent is unavailable."));
  });
});
