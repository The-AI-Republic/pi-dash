// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { agentToolsEnabled, type Profile } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { sessionKeys } from "../../core/session/queries.js";

const { updateProfileMock } = vi.hoisted(() => ({ updateProfileMock: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, updateProfile: updateProfileMock };
});

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

import { notifyError } from "../../shared/shell/index.js";
import {
  AGENT_TOOLS_SAVE_FAILED_MESSAGE,
  AGENT_TOOLS_SAVE_FAILED_TITLE,
  agentToolsPatch,
  applyAgentToolsFlip,
  useAgentToolsEnabled,
  useSetAgentToolsEnabled,
} from "./api/mutations.js";

function profileWith(enabled: boolean, extra: Record<string, unknown> = {}): Profile {
  return {
    settings: { ...extra, openhub: { apps_enabled: enabled } },
  } as unknown as Profile;
}

function flagOf(queryClient: QueryClient): boolean {
  const profile = queryClient.getQueryData<Profile>(sessionKeys.profile);
  if (profile === undefined) throw new Error("expected a cached profile");
  return agentToolsEnabled(profile.settings);
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("agent tools patch", () => {
  it("carries the flip in the catalog namespace (CLOUD-029)", () => {
    expect(agentToolsPatch(true)).toEqual({ settings: { openhub: { apps_enabled: true } } });
    expect(agentToolsPatch(false)).toEqual({ settings: { openhub: { apps_enabled: false } } });
  });
});

describe("agent tools flip", () => {
  it("reads an unset profile through untouched", () => {
    expect(applyAgentToolsFlip(undefined, true)).toBeUndefined();
  });

  it("starts a fresh namespace when the profile has none", () => {
    const flipped = applyAgentToolsFlip({ settings: {} } as unknown as Profile, true);
    expect(flipped?.settings).toEqual({ openhub: { apps_enabled: true } });
  });

  it("starts a fresh namespace over a non-object value", () => {
    const flipped = applyAgentToolsFlip({ settings: { openhub: "yes" } } as unknown as Profile, true);
    expect(flipped?.settings).toEqual({ openhub: { apps_enabled: true } });
  });

  it("starts a fresh namespace over a null value", () => {
    const flipped = applyAgentToolsFlip({ settings: { openhub: null } } as unknown as Profile, true);
    expect(flipped?.settings).toEqual({ openhub: { apps_enabled: true } });
  });

  it("merges over siblings without dropping them", () => {
    const previous = profileWith(false, { other: { keep: 1 } });
    (previous.settings["openhub"] as Record<string, unknown>)["draft"] = "keep-me";
    const flipped = applyAgentToolsFlip(previous, true);
    expect(flipped?.settings).toEqual({
      other: { keep: 1 },
      openhub: { apps_enabled: true, draft: "keep-me" },
    });
  });
});

describe("agent tools write", () => {
  it("applies optimistically and takes the server row (CLOUD-029)", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.profile, profileWith(false));
    let resolvePatch!: (value: unknown) => void;
    updateProfileMock.mockReturnValueOnce(
      new Promise((resolve) => {
        resolvePatch = resolve;
      })
    );
    const { result } = renderHook(() => useSetAgentToolsEnabled(), { wrapper: wrapper(queryClient) });

    let done!: Promise<Profile>;
    await act(async () => {
      done = result.current.mutateAsync(true);
    });
    // The UI flips before the PATCH lands, with the write in flight.
    expect(flagOf(queryClient)).toBe(true);
    const inFlight = queryClient.getMutationCache().getAll();
    expect(inFlight).toHaveLength(1);
    expect(inFlight[0]?.state.status).toBe("pending");

    const saved = profileWith(true);
    await act(async () => {
      resolvePatch(saved);
      await done;
    });
    expect(queryClient.getQueryData(sessionKeys.profile)).toEqual(saved);
    expect(updateProfileMock).toHaveBeenCalledWith(expect.anything(), {
      settings: { openhub: { apps_enabled: true } },
    });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls back with an error notice when the server refuses (CLOUD-029)", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.profile, profileWith(false));
    updateProfileMock.mockRejectedValueOnce(new Error("nope"));
    const { result } = renderHook(() => useSetAgentToolsEnabled(), { wrapper: wrapper(queryClient) });

    await act(async () => {
      await expect(result.current.mutateAsync(true)).rejects.toThrow("nope");
    });
    expect(flagOf(queryClient)).toBe(false);
    expect(notifyError).toHaveBeenCalledWith(AGENT_TOOLS_SAVE_FAILED_TITLE, AGENT_TOOLS_SAVE_FAILED_MESSAGE);
  });
});

describe("agent tools read", () => {
  it("reads the flag once the profile loads", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.profile, profileWith(true));
    const { result } = renderHook(() => useAgentToolsEnabled(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current).toBe(true));
  });

  it("reads off for an unset flag", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.profile, { settings: {} } as unknown as Profile);
    const { result } = renderHook(() => useAgentToolsEnabled(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current).toBe(false));
  });

  it("reads undefined before the profile loads", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAgentToolsEnabled(), { wrapper: wrapper(queryClient) });
    expect(result.current).toBeUndefined();
  });
});
