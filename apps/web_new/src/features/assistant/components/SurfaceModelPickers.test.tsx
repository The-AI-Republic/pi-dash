// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface model pickers for Settings (NEWPIDASH-394, design section
// 7.4): one picker per surface with its effective choice, the fallback
// notice when the saved choice is set but unused, and PUT writes scoped
// to the picker's own surface.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notified = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { buildMergedModelChoices } from "../lib/openhubPicker.js";
import { SurfaceModelPickers, surfacePickerLabel, type SurfacePickerSpec } from "./SurfaceModelPickers.js";

const BYOK_ID = "223e4567-e89b-12d3-a456-426614174002";

const CATALOG = {
  models: ["moonshotai/kimi-k3", "openhub_free_llm"],
  labels: { "moonshotai/kimi-k3": "Kimi K3" },
  descriptions: {},
};

const KEYS = [
  {
    connectionId: BYOK_ID,
    models: ["gpt-6", "gpt-6.2"],
    baseUrl: "https://llm.example.com/v1",
    providerKind: "openai_compatible",
  },
];

const FULL = buildMergedModelChoices(CATALOG, KEYS);
const CATALOG_ONLY = buildMergedModelChoices(CATALOG, KEYS, { includeByok: false });

const SPECS: SurfacePickerSpec[] = [
  { surface: "web_assistant", label: "Web assistant", choices: FULL },
  { surface: "desktop_assistant", label: "Desktop assistant", choices: FULL },
  { surface: "agent", label: "Pi Dash Agent", choices: CATALOG_ONLY },
];

interface SurfaceState {
  saved: { model: string; connection_id: string | null } | null;
  effective: { model: string; connection_id: string | null } | null;
  fallback_reason: string | null;
}

const selectionState: Record<"web_assistant" | "desktop_assistant" | "agent", SurfaceState> = {
  web_assistant: {
    saved: { model: "moonshotai/kimi-k3", connection_id: null },
    effective: { model: "moonshotai/kimi-k3", connection_id: null },
    fallback_reason: null,
  },
  desktop_assistant: {
    saved: { model: "modelC", connection_id: null },
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "model_removed",
  },
  agent: { saved: null, effective: null, fallback_reason: "lane_unavailable" },
};

const SELECTION = "/api/users/me/ai-assistant/model-selection/";

const calls: Array<{ method: string; url: string; body: unknown }> = [];
let putStatus = 200;

function stubTransport(): void {
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const call = {
      method: init.method ?? "GET",
      url,
      body: typeof init.body === "string" && init.body !== "" ? (JSON.parse(init.body) as unknown) : null,
    };
    calls.push(call);
    let status = 200;
    let body: unknown = null;
    if (call.method === "GET" && url === `https://api.test${SELECTION}`) {
      body = { ...selectionState };
    } else if (call.method === "PUT" && url.startsWith(`https://api.test${SELECTION}`)) {
      status = putStatus;
      if (status === 200) {
        const input = call.body as { model: string; connection_id: string | null };
        const surface = url.slice(`https://api.test${SELECTION}`.length, -1) as keyof typeof selectionState;
        selectionState[surface] = {
          saved: { model: input.model, connection_id: input.connection_id },
          effective: { model: input.model, connection_id: input.connection_id },
          fallback_reason: null,
        };
        body = { ...selectionState[surface] };
      } else {
        body = { error: "model_not_available", detail: "Model not available." };
      }
    } else {
      throw new Error(`unexpected call ${call.method} ${url}`);
    }
    return {
      status,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  }) as never);
}

function renderSection(surfaces: readonly SurfacePickerSpec[] = SPECS) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const rendered = render(
    <QueryClientProvider client={queryClient}>
      <SurfaceModelPickers workspaceSlug="acme" surfaces={surfaces} flags={null} />
    </QueryClientProvider>
  );
  return { ...rendered, queryClient };
}

function resetSelection(): void {
  selectionState.web_assistant = {
    saved: { model: "moonshotai/kimi-k3", connection_id: null },
    effective: { model: "moonshotai/kimi-k3", connection_id: null },
    fallback_reason: null,
  };
  selectionState.desktop_assistant = {
    saved: { model: "modelC", connection_id: null },
    effective: { model: "openhub_free_llm", connection_id: null },
    fallback_reason: "model_removed",
  };
  selectionState.agent = { saved: null, effective: null, fallback_reason: "lane_unavailable" };
}

afterEach(() => {
  calls.length = 0;
  putStatus = 200;
  resetSelection();
  notified.error.mockReset();
  resetAppClient();
});

describe("surfacePickerLabel", () => {
  it("names each surface", () => {
    expect(surfacePickerLabel("web_assistant")).toBe("Web assistant");
    expect(surfacePickerLabel("desktop_assistant")).toBe("Desktop assistant");
    expect(surfacePickerLabel("agent")).toBe("Pi Dash Agent");
  });
});

describe("SurfaceModelPickers", () => {
  it("shows each surface's effective choice and the fallback notice", async () => {
    stubTransport();
    renderSection();
    expect(await screen.findByRole("combobox", { name: "Web assistant" })).toHaveValue("catalog:moonshotai/kimi-k3");
    expect(await screen.findByRole("combobox", { name: "Desktop assistant" })).toHaveValue("catalog:openhub_free_llm");
    expect(screen.getByTestId("surface-model-fallback-desktop_assistant")).toHaveTextContent(
      "modelC is no longer available. Using openhub_free_llm."
    );
    expect(screen.queryByTestId("surface-model-fallback-web_assistant")).toBeNull();
  });

  it("offers catalog models only on the agent surface", async () => {
    stubTransport();
    renderSection();
    const agent = await screen.findByRole("combobox", { name: "Pi Dash Agent" });
    expect(agent).toHaveValue("");
    expect(Array.from(agent.querySelectorAll("option")).map((option) => option.textContent)).toEqual([
      "No model available",
      "Kimi K3",
      "openhub_free_llm (default)",
    ]);
  });

  it("writes a desktop pick through PUT desktop_assistant only", async () => {
    stubTransport();
    renderSection();
    const desktop = await screen.findByRole("combobox", { name: "Desktop assistant" });
    fireEvent.change(desktop, { target: { value: `byok:${BYOK_ID}:gpt-6` } });
    await waitFor(() => expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1));
    const put = calls.find((call) => call.method === "PUT");
    expect(put?.url).toBe(`https://api.test${SELECTION}desktop_assistant/`);
    expect(put?.body).toEqual({ model: "gpt-6", connection_id: BYOK_ID });
    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "Desktop assistant" })).toHaveValue(`byok:${BYOK_ID}:gpt-6`)
    );
    expect(screen.getByRole("combobox", { name: "Web assistant" })).toHaveValue("catalog:moonshotai/kimi-k3");
  });

  it("keeps the selection and notifies when the write is refused", async () => {
    putStatus = 400;
    stubTransport();
    renderSection();
    const desktop = await screen.findByRole("combobox", { name: "Desktop assistant" });
    fireEvent.change(desktop, { target: { value: "catalog:moonshotai/kimi-k3" } });
    await waitFor(() => expect(notified.error).toHaveBeenCalled());
    expect(screen.getByRole("combobox", { name: "Desktop assistant" })).toHaveValue("catalog:openhub_free_llm");
  });

  it("skips a surface with no choices", async () => {
    stubTransport();
    renderSection([{ surface: "web_assistant", label: "Web assistant", choices: [] }]);
    await waitFor(() => expect(calls.filter((call) => call.method === "GET")).toHaveLength(1));
    expect(screen.queryByRole("combobox", { name: "Web assistant" })).toBeNull();
  });
});
