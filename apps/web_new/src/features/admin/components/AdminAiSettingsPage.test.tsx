// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI settings page (ADM-046, ADM-047, ADM-048): skeleton until
// configurations load, prefilled optional form with always-enabled
// save, success notice on save, log-only failure, show/hide toggle,
// helper and callout links, and a retry when the read fails.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { AI_SETTINGS_SAVED_MESSAGE, AI_SETTINGS_SAVED_TITLE } from "../aiSettings.js";
import { AdminAiSettingsPage } from "./AdminAiSettingsPage.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

function row(key: string, value: string | null): Record<string, unknown> {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "ai",
    is_encrypted: false,
    source: "db",
    is_managed: true,
  };
}

interface StubCall {
  method: string;
  url: string;
  body: unknown;
}

interface StubOptions {
  getStatus?: number;
  getRows?: Array<Record<string, unknown>>;
  patchStatus?: number;
  patchGate?: Promise<void>;
  failGetTimes?: number;
}

function stubClient(options: StubOptions = {}): { calls: StubCall[]; getCount: () => number } {
  const calls: StubCall[] = [];
  let gets = 0;
  const failGetTimes = options.failGetTimes ?? 0;
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: unknown }) => {
    const method = init?.method ?? "GET";
    calls.push({ method, url, body: init?.body });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (method === "PATCH" && url.endsWith("/api/instances/configurations/")) {
      if (options.patchGate) await options.patchGate;
      const status = options.patchStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status >= 200 && status < 300
            ? JSON.stringify(options.getRows ?? [])
            : JSON.stringify({ detail: "cannot save" }),
      };
    }
    if (method === "GET" && url.endsWith("/api/instances/configurations/")) {
      gets += 1;
      if (gets <= failGetTimes || (options.getStatus ?? 200) >= 400) {
        const status = options.getStatus ?? 500;
        return { status, url, headers: { get: () => null }, text: async () => JSON.stringify({ detail: "boom" }) };
      }
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(options.getRows ?? []),
      };
    }
    throw new Error(`unexpected ${method} ${url}`);
  }) as never);
  return { calls, getCount: () => gets };
}

function renderPage(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AdminAiSettingsPage />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.mocked(notifySuccess).mockClear();
  vi.mocked(notifyError).mockClear();
});

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("loading and failure", () => {
  it("shows the skeleton until configurations load, then yields to the form", async () => {
    stubClient({ getRows: [row("LLM_MODEL", "gpt-4o"), row("LLM_API_KEY", "sk-live")] });
    renderPage();
    const skeleton = screen.getByTestId("admin-ai-skeleton");
    expect(skeleton).toHaveAttribute("role", "status");
    expect(skeleton.children.length).toBeGreaterThan(0);
    expect(screen.queryByTestId("admin-ai-form")).not.toBeInTheDocument();
    await screen.findByTestId("admin-ai-form");
    expect(screen.queryByTestId("admin-ai-skeleton")).not.toBeInTheDocument();
  });

  it("offers a retry when the read fails, and the retry loads the form", async () => {
    const user = userEvent.setup();
    stubClient({ getRows: [row("LLM_MODEL", "gpt-4o")], failGetTimes: 1 });
    renderPage();
    await screen.findByTestId("admin-ai-error");
    expect(screen.queryByTestId("admin-ai-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("admin-ai-form")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("admin-ai-retry"));
    await screen.findByTestId("admin-ai-form");
    expect(screen.queryByTestId("admin-ai-error")).not.toBeInTheDocument();
  });
});

describe("form and save", () => {
  it("prefills the stored values with the save enabled", async () => {
    stubClient({ getRows: [row("LLM_MODEL", "gpt-4o"), row("LLM_API_KEY", "sk-live")] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    expect(screen.getByLabelText("Model")).toHaveValue("gpt-4o");
    expect(screen.getByLabelText("API key")).toHaveValue("sk-live");
    expect(screen.getByTestId("admin-ai-save")).toBeEnabled();
  });

  it("persists both values with a success notice and re-reads the rows", async () => {
    const user = userEvent.setup();
    const { calls, getCount } = stubClient({ getRows: [row("LLM_MODEL", ""), row("LLM_API_KEY", "")] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    await user.clear(screen.getByLabelText("Model"));
    await user.type(screen.getByLabelText("Model"), "gpt-probe");
    await user.clear(screen.getByLabelText("API key"));
    await user.type(screen.getByLabelText("API key"), "sk-probe");
    await user.click(screen.getByTestId("admin-ai-save"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(AI_SETTINGS_SAVED_TITLE, AI_SETTINGS_SAVED_MESSAGE));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain("/api/instances/configurations/");
    expect(typeof patch?.body === "string" ? JSON.parse(patch.body) : patch?.body).toEqual({
      LLM_MODEL: "gpt-probe",
      LLM_API_KEY: "sk-probe",
    });
    await waitFor(() => expect(getCount()).toBeGreaterThan(1));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("allows an empty save with the save always enabled", async () => {
    const user = userEvent.setup();
    const { calls } = stubClient({ getRows: [row("LLM_MODEL", "gpt-4o"), row("LLM_API_KEY", "sk-live")] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    await user.clear(screen.getByLabelText("Model"));
    await user.clear(screen.getByLabelText("API key"));
    const save = screen.getByTestId("admin-ai-save");
    expect(save).toBeEnabled();
    await user.click(save);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    const patch = calls.find((call) => call.method === "PATCH");
    expect(typeof patch?.body === "string" ? JSON.parse(patch.body) : patch?.body).toEqual({
      LLM_MODEL: "",
      LLM_API_KEY: "",
    });
  });

  it("swaps the save label while the write is in flight", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const patchGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubClient({ getRows: [], patchGate });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    await user.click(screen.getByTestId("admin-ai-save"));
    await screen.findByRole("button", { name: "Saving…" });
    expect(screen.getByTestId("admin-ai-save")).toBeDisabled();
    release();
    await screen.findByRole("button", { name: "Save changes" });
  });

  it("logs a failed save to the console with no notice", async () => {
    const user = userEvent.setup();
    const errors: unknown[] = [];
    vi.stubGlobal("console", { ...console, error: (...args: unknown[]) => void errors.push(args) });
    stubClient({ getRows: [], patchStatus: 500 });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    await user.type(screen.getByLabelText("Model"), "gpt-fail");
    await user.click(screen.getByTestId("admin-ai-save"));
    await waitFor(() => expect(errors.length).toBeGreaterThan(0));
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });
});

describe("key visibility", () => {
  it("starts hidden, flips to text and back, and keeps the value", async () => {
    const user = userEvent.setup();
    stubClient({ getRows: [row("LLM_API_KEY", "sk-live")] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    const key = screen.getByLabelText("API key");
    const toggle = screen.getByTestId("admin-ai-key-toggle");
    expect(key).toHaveAttribute("type", "password");
    expect(toggle).toHaveAttribute("tabindex", "-1");
    await user.click(toggle);
    expect(key).toHaveAttribute("type", "text");
    expect(toggle).toHaveAttribute("aria-label", "Hide API key");
    await user.click(toggle);
    expect(key).toHaveAttribute("type", "password");
    expect(toggle).toHaveAttribute("aria-label", "Show API key");
    expect(key).toHaveValue("sk-live");
  });
});

describe("helper links and callout", () => {
  it("points the helper links at the vendor docs", async () => {
    stubClient({ getRows: [] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    expect(screen.getByTestId("admin-ai-model-help")).toHaveAttribute(
      "href",
      expect.stringContaining("platform.openai.com/docs/models")
    );
    expect(screen.getByTestId("admin-ai-key-help")).toHaveAttribute(
      "href",
      expect.stringContaining("platform.openai.com/api-keys")
    );
  });

  it("shows the messaging-only callout with the contact link", async () => {
    stubClient({ getRows: [] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    const callout = screen.getByTestId("admin-ai-callout");
    expect(callout).toBeInTheDocument();
    expect(screen.getByTestId("admin-ai-callout-link")).toHaveAttribute(
      "href",
      expect.stringContaining("airepublic.com/contact")
    );
    expect(callout.querySelector("input,button")).toBeNull();
  });

  it("titles the page like the sidebar entry", async () => {
    stubClient({ getRows: [] });
    renderPage();
    await screen.findByTestId("admin-ai-form");
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Artificial intelligence");
  });
});
