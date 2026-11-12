// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useUpdateProjectLimit, useUpdateProjectNavMode, type ProjectNavMode } from "./displayPreferences.js";
import { useProjectListCap } from "./tabPreferences.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";

function propsPayload(mode: string, limit: number) {
  return {
    id: UUID,
    workspace: UUID,
    user: UUID,
    filters: {},
    display_filters: {},
    display_properties: {},
    rich_filters: {},
    navigation_project_limit: limit,
    navigation_control_preference: mode,
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

interface StubOptions {
  mode?: string;
  limit?: number;
  patchStatus?: number;
  hangGet?: boolean;
}

function stubClient(options: StubOptions = {}) {
  let serverMode = options.mode ?? "ACCORDION";
  let serverLimit = options.limit ?? 0;
  const calls: Array<{ url: string; method: string; body: string }> = [];
  const client = createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (init.method === "PATCH") {
      const status = options.patchStatus ?? 200;
      if (status !== 200) {
        return { status, url, headers: { get: () => null }, text: async () => "{}" };
      }
      const sent = JSON.parse(init.body ?? "{}") as {
        navigation_control_preference?: string;
        navigation_project_limit?: number;
      };
      if (sent.navigation_control_preference !== undefined) serverMode = sent.navigation_control_preference;
      if (sent.navigation_project_limit !== undefined) serverLimit = sent.navigation_project_limit;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(propsPayload(serverMode, serverLimit)),
      };
    }
    if (options.hangGet === true) return new Promise<never>(() => undefined);
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(propsPayload(serverMode, serverLimit)),
    };
  }) as never);
  return { client, calls };
}

describe("display preference hooks", () => {
  function renderHandle(options: StubOptions = {}) {
    const { calls } = stubClient(options);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    let setMode: ((mode: ProjectNavMode) => void) | null = null;
    let setLimit: ((limit: number) => void) | null = null;
    function Probe(): React.ReactElement {
      const cap = useProjectListCap("acme");
      const mode = useUpdateProjectNavMode("acme");
      const limit = useUpdateProjectLimit("acme");
      setMode = mode.setMode;
      setLimit = limit.setLimit;
      return <p>{`cap:${cap ?? "uncapped"}|saving:${mode.isSaving || limit.isSaving}`}</p>;
    }
    render(
      <QueryClientProvider client={queryClient}>
        <Probe />
      </QueryClientProvider>
    );
    return {
      calls,
      text: () => screen.getByText(/^cap:/).textContent ?? "",
      mode: (next: ProjectNavMode) => setMode?.(next),
      limit: (next: number) => setLimit?.(next),
    };
  }

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetAppClient();
  });

  it("reads the effective cap, uncapped by default", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("cap:uncapped|saving:false"));
  });

  it("persists the mode with its field only", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("cap:uncapped|saving:false"));
    h.mode("TABBED");
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain("/api/workspaces/acme/user-properties/");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ navigation_control_preference: "TABBED" });
  });

  it("persists the cap and surfaces it once stored", async () => {
    const h = renderHandle();
    await waitFor(() => expect(h.text()).toBe("cap:uncapped|saving:false"));
    h.limit(2);
    await waitFor(() => expect(h.text()).toBe("cap:2|saving:false"));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ navigation_project_limit: 2 });
  });

  it("reverts the optimistic cap and toasts when the save fails", async () => {
    const h = renderHandle({ patchStatus: 500 });
    await waitFor(() => expect(h.text()).toBe("cap:uncapped|saving:false"));
    h.limit(2);
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't save display preferences", expect.any(String))
    );
    expect(h.text()).toBe("cap:uncapped|saving:false");
  });

  it("still sends the write while the first read hangs", async () => {
    const h = renderHandle({ hangGet: true });
    await screen.findByText("cap:uncapped|saving:false");
    h.mode("TABBED");
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(notifyError).not.toHaveBeenCalled();
  });
});
