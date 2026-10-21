// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository tab (PSET-050–052): skeleton, bound card content, admin-only
// sync and unbind, confirm-free unbind with the mirrored-issues notice,
// the unbound guidance card and the absence of branch/direction pickers.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import {
  cadenceLine,
  providerLabel,
  REPOSITORY_GUIDANCE_BODY,
  RepositorySyncCard,
  UNBOUND_BODY,
  UNBOUND_TITLE,
} from "./index.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

function binding(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    bound: true,
    id: "66666666-6666-4666-8666-666666666666",
    provider: "github",
    provider_account_id: null,
    host_url: "https://github.com",
    repository: {
      id: "repo-1",
      provider: "github",
      host_url: "https://github.com",
      namespace: "parity",
      name: "parity-seed",
      full_name: "parity/parity-seed",
      web_url: "https://github.com/parity/parity-seed",
      clone_url_http: "https://github.com/parity/parity-seed.git",
      clone_url_ssh: "git@github.com:parity/parity-seed.git",
      default_branch: "main",
      private: false,
    },
    is_sync_enabled: false,
    clone_auth_mode: "public",
    last_synced_at: null,
    last_sync_error: "",
    degraded: false,
    degraded_reason: null,
    ...overrides,
  };
}

interface StubOptions {
  initial?: Record<string, unknown>;
  hold?: Promise<void>;
  getStatus?: number;
  patchStatus?: number;
  deleteStatus?: number;
  holdDelete?: Promise<void>;
}

function stubApi(options: StubOptions = {}) {
  const calls: { method: string; body: unknown }[] = [];
  let current = options.initial ?? binding();
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (!url.endsWith(`/api/workspaces/${WORKSPACE}/projects/${PROJECT}/repository/`)) {
      throw new Error(`unexpected fetch ${method} ${url}`);
    }
    calls.push({ method, body });
    if (method === "GET") {
      if (options.hold !== undefined) await options.hold;
      if ((options.getStatus ?? 200) !== 200) return jsonResponse(url, options.getStatus ?? 500, {});
      return jsonResponse(url, 200, current);
    }
    if (method === "PATCH") {
      if ((options.patchStatus ?? 200) !== 200) return jsonResponse(url, options.patchStatus ?? 500, {});
      current = { ...current, is_sync_enabled: (body as { enabled: boolean }).enabled };
      return jsonResponse(url, 200, current);
    }
    if (method === "DELETE") {
      if (options.holdDelete !== undefined) await options.holdDelete;
      if ((options.deleteStatus ?? 200) !== 200) return jsonResponse(url, options.deleteStatus ?? 500, {});
      current = { bound: false };
      return jsonResponse(url, 200, current);
    }
    throw new Error(`unexpected method ${method}`);
  }) as never);
  return { calls };
}

function renderCard(canManage = true) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RepositorySyncCard workspaceSlug={WORKSPACE} projectId={PROJECT} canManage={canManage} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("repository card helpers", () => {
  it("names the provider family and falls back for unknown ones", () => {
    expect(providerLabel("github")).toBe("GitHub");
    expect(providerLabel("gitlab")).toBe("GitLab");
    expect(providerLabel("bitbucket")).toBe("Git provider");
  });

  it("appends the last success to the fixed cadence", () => {
    expect(cadenceLine(null)).toBe("Sync issues every 4 hours");
    expect(cadenceLine("2024-05-06T07:08:00Z")).toMatch(/^Sync issues every 4 hours — last sync /);
    expect(cadenceLine("2024-05-06T07:08:00Z")).toContain("2024");
  });
});

describe("RepositorySyncCard", () => {
  it("shows a skeleton while the binding read is held", async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubApi({ hold: held });
    renderCard();
    expect(screen.getByRole("status", { name: "Loading repository binding" })).toBeInTheDocument();
    release();
    await screen.findByTestId("repository-bound-card");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("renders the bound card with provider, outbound link, contract and cadence", async () => {
    stubApi({ initial: binding({ last_synced_at: "2024-05-06T07:08:00Z" }) });
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    expect(within(card).getByRole("heading", { name: "GitHub" })).toBeInTheDocument();
    const link = within(card).getByRole("link", { name: /parity\/parity-seed/ });
    expect(link).toHaveAttribute("href", "https://github.com/parity/parity-seed");
    expect(link).toHaveAttribute("target", "_blank");
    expect(card).toHaveTextContent("Mirrored issues are read-only");
    expect(card).toHaveTextContent("Workflow fields stay editable");
    expect(within(card).getByText(/^Sync issues every 4 hours/)).toHaveTextContent("last sync");
    expect(card).not.toHaveTextContent("Last error:");
  });

  it("shows the failure line only when an error exists", async () => {
    stubApi({ initial: binding({ last_sync_error: "provider token expired" }) });
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    expect(card).toHaveTextContent("Last error: provider token expired");
  });

  it("disables the sync switch and unbind for members", async () => {
    stubApi();
    renderCard(false);
    const card = await screen.findByTestId("repository-bound-card");
    expect(within(card).getByRole("switch")).toHaveAttribute("aria-disabled", "true");
    expect(within(card).getByRole("button", { name: "Unbind" })).toBeDisabled();
  });

  it("flips the sync flag twice with the binding intact", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    const control = within(card).getByRole("switch");
    expect(control).toHaveAttribute("aria-checked", "false");
    await user.click(control);
    await waitFor(() => expect(control).toHaveAttribute("aria-checked", "true"));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([{ enabled: true }]);
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);

    await user.click(control);
    await waitFor(() => expect(control).toHaveAttribute("aria-checked", "false"));
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([
      { enabled: true },
      { enabled: false },
    ]);
    expect(screen.getByTestId("repository-bound-card")).toBeInTheDocument();
  });

  it("keeps the switch position and notices an error when the write fails", async () => {
    const user = userEvent.setup();
    stubApi({ patchStatus: 500 });
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    const control = within(card).getByRole("switch");
    await user.click(control);
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(control).toHaveAttribute("aria-checked", "false");
  });

  it("unbinds with no confirmation and lands on the guidance card", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi();
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    await user.click(within(card).getByRole("button", { name: "Unbind" }));
    await waitFor(() =>
      expect(notifySuccess).toHaveBeenCalledWith(
        UNBOUND_TITLE,
        expect.stringContaining("Mirrored issues remain editable")
      )
    );
    expect(UNBOUND_BODY).toBe("Mirrored issues remain editable.");
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(await screen.findByTestId("repository-guidance-card")).toBeInTheDocument();
  });

  it("blocks both controls while the unbind is in flight", async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    const user = userEvent.setup();
    stubApi({ holdDelete: held });
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    const unbind = within(card).getByRole("button", { name: "Unbind" });
    await user.click(unbind);
    await waitFor(() => expect(unbind).toBeDisabled());
    expect(within(card).getByRole("switch")).toHaveAttribute("aria-disabled", "true");
    release();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
  });

  it("keeps the bound card with an error notice when unbind fails", async () => {
    const user = userEvent.setup();
    stubApi({ deleteStatus: 500 });
    renderCard();
    const card = await screen.findByTestId("repository-bound-card");
    await user.click(within(card).getByRole("button", { name: "Unbind" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(screen.getByTestId("repository-bound-card")).toBeInTheDocument();
  });

  it("guides an unbound project to General settings with no pickers", async () => {
    stubApi({ initial: { bound: false } });
    renderCard();
    const card = await screen.findByTestId("repository-guidance-card");
    expect(within(card).getByRole("heading", { name: "Repository sync" })).toBeInTheDocument();
    expect(card).toHaveTextContent(REPOSITORY_GUIDANCE_BODY);
    expect(card).toHaveTextContent("in General Settings");
    expect(card.textContent ?? "").not.toMatch(/branch|direction/i);
    expect(within(card).queryByRole("combobox")).not.toBeInTheDocument();
    expect(within(card).queryByRole("switch")).not.toBeInTheDocument();
  });

  it("reports a failed read instead of guessing a binding", async () => {
    stubApi({ getStatus: 500 });
    renderCard();
    expect(await screen.findByRole("alert")).toHaveTextContent("The repository status could not be loaded");
  });
});
