// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notificationKeys } from "../api/keys.js";
import { EMAIL_PREFS_FAILED_TITLE, EMAIL_PREFS_SAVED_TITLE } from "../api/preferences.js";
import {
  COMPLETED_ONLY_ROW,
  EMAIL_PREFERENCE_ROWS,
  EMAIL_PREFS_HEADING,
  EMAIL_PREFS_INTRO,
  EMAIL_PREFS_LOAD_FAILED,
  EMAIL_PREFS_LOADING_LABEL,
  EmailPreferencesPage,
  pendingFlipTopic,
} from "./EmailPreferencesPage.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

const PREFS_PATH = "/api/users/me/notification-preferences/";

function prefsRow(overrides: Record<string, unknown> = {}) {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    user: "22222222-2222-4222-8222-222222222222",
    workspace: null,
    project: null,
    property_change: true,
    state_change: true,
    comment: false,
    mention: true,
    issue_completed: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  } as TransportResponse;
}

interface StubOptions {
  row?: Record<string, unknown>;
  getStatus?: number;
  hangGet?: boolean;
  patchStatus?: number;
  onPatch?: (sent: Record<string, unknown>) => void;
}

/** Stub API: CSRF token, prefs GET from a server row, prefs PATCH with one key. */
function stubApi(options: StubOptions = {}) {
  let server = prefsRow(options.row);
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return jsonResponse(url, 200, { csrf_token: "csrf" });
    }
    if (!url.endsWith(PREFS_PATH)) throw new Error(`unexpected fetch ${url}`);
    if (init.method === "PATCH") {
      const sent = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
      options.onPatch?.(sent);
      if ((options.patchStatus ?? 200) !== 200) {
        return jsonResponse(url, options.patchStatus ?? 500, { detail: "parity prefs failure" });
      }
      server = { ...server, ...sent };
      return jsonResponse(url, 200, server);
    }
    if (options.hangGet === true) return new Promise<never>(() => undefined);
    if ((options.getStatus ?? 200) !== 200) {
      return jsonResponse(url, options.getStatus ?? 500, { detail: "parity prefs load failure" });
    }
    return jsonResponse(url, 200, server);
  }) as never);
  return {
    serverRow: () => server,
  };
}

function testClient() {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderPage(queryClient: QueryClient) {
  return render(
    <QueryClientProvider client={queryClient}>
      <EmailPreferencesPage />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("pendingFlipTopic", () => {
  it("returns nothing when no save runs", () => {
    expect(pendingFlipTopic({ isPending: false, variables: { topic: "comment" } })).toBeUndefined();
  });

  it("returns nothing while pending without variables", () => {
    expect(pendingFlipTopic({ isPending: true, variables: undefined })).toBeUndefined();
  });

  it("returns the in-flight topic", () => {
    expect(pendingFlipTopic({ isPending: true, variables: { topic: "mention" } })).toBe("mention");
  });
});

describe("EmailPreferencesPage", () => {
  it("shows a loader until the settings arrive", async () => {
    stubApi({ hangGet: true });
    renderPage(testClient());

    expect(screen.getByTestId("email-prefs-loader")).toBeInTheDocument();
    expect(screen.getByText(EMAIL_PREFS_LOADING_LABEL)).toBeInTheDocument();
    expect(screen.queryByTestId("email-prefs-form")).not.toBeInTheDocument();
  });

  it("reports a failed load with no retry control", async () => {
    stubApi({ getStatus: 500 });
    renderPage(testClient());

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(EMAIL_PREFS_LOAD_FAILED));
    expect(screen.queryByTestId("email-prefs-form")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("renders one toggle per topic matching the server state", async () => {
    stubApi();
    renderPage(testClient());

    await waitFor(() => expect(screen.getByTestId("email-prefs-form")).toBeInTheDocument());
    expect(screen.getByRole("heading", { name: EMAIL_PREFS_HEADING })).toBeInTheDocument();
    expect(screen.getByText(EMAIL_PREFS_INTRO)).toBeInTheDocument();

    const switches = screen.getAllByRole("switch");
    expect(switches.map((node) => node.getAttribute("aria-label"))).toEqual([
      "Property changes",
      "State changes",
      "Completed items only",
      "Comments",
      "Mentions",
    ]);
    expect(screen.getByRole("switch", { name: "Property changes" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "State changes" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Completed items only" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "Comments" })).not.toBeChecked();
    expect(screen.getByRole("switch", { name: "Mentions" })).toBeChecked();
  });

  it("nests the completed-only toggle under state changes", async () => {
    stubApi();
    renderPage(testClient());

    await waitFor(() => expect(screen.getByTestId("email-prefs-form")).toBeInTheDocument());
    const nest = screen.getByTestId("email-prefs-completed-nest");
    expect(within(nest).getByRole("switch", { name: COMPLETED_ONLY_ROW.heading })).toBeInTheDocument();
    expect(within(nest).getAllByRole("switch")).toHaveLength(1);
    // The nest sits between the state row and the comment row.
    const form = screen.getByTestId("email-prefs-form");
    const headings = within(form)
      .getAllByRole("heading")
      .map((node) => node.textContent);
    expect(headings).toEqual([
      EMAIL_PREFERENCE_ROWS[0]?.heading,
      EMAIL_PREFERENCE_ROWS[1]?.heading,
      COMPLETED_ONLY_ROW.heading,
      EMAIL_PREFERENCE_ROWS[2]?.heading,
      EMAIL_PREFERENCE_ROWS[3]?.heading,
    ]);
  });

  it("saves a flip instantly with a success toast", async () => {
    const user = userEvent.setup();
    const seen: Array<Record<string, unknown>> = [];
    const api = stubApi({ onPatch: (sent) => seen.push(sent) });
    renderPage(testClient());

    const toggle = await screen.findByRole("switch", { name: "Comments" });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(EMAIL_PREFS_SAVED_TITLE));
    expect(seen).toEqual([{ comment: true }]);
    expect(api.serverRow().comment).toBe(true);
    expect(screen.getByRole("switch", { name: "Comments" })).toBeChecked();
  });

  it("springs the toggle back with an error toast when the save fails", async () => {
    const user = userEvent.setup();
    const api = stubApi({ patchStatus: 500 });
    renderPage(testClient());

    const toggle = await screen.findByRole("switch", { name: "Comments" });
    await user.click(toggle);

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(EMAIL_PREFS_FAILED_TITLE, expect.any(String)));
    expect(api.serverRow().comment).toBe(false);
    expect(screen.getByRole("switch", { name: "Comments" })).not.toBeChecked();
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("disables the toggle with a save in flight", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return jsonResponse(url, 200, { csrf_token: "csrf" });
      }
      if (init.method !== "PATCH") return jsonResponse(url, 200, prefsRow());
      await gate;
      return jsonResponse(url, 200, prefsRow({ comment: true }));
    }) as never);
    renderPage(testClient());

    const toggle = await screen.findByRole("switch", { name: "Comments" });
    await user.click(toggle);
    // Base UI marks disabled switches with aria-disabled, not the disabled
    // attribute, so jest-dom's toBeDisabled does not apply here.
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Comments" }).getAttribute("aria-disabled")).toBe("true")
    );
    expect(screen.getByRole("switch", { name: "Mentions" }).getAttribute("aria-disabled")).not.toBe("true");

    await act(async () => {
      release();
    });
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Comments" }).getAttribute("aria-disabled")).not.toBe("true")
    );
    expect(screen.getByRole("switch", { name: "Comments" })).toBeChecked();
  });

  it("re-syncs the toggles when fresh settings arrive", async () => {
    stubApi();
    const queryClient = testClient();
    renderPage(queryClient);

    await screen.findByTestId("email-prefs-form");
    expect(screen.getByRole("switch", { name: "Comments" })).not.toBeChecked();

    act(() => {
      queryClient.setQueryData(notificationKeys.emailPreferences, prefsRow({ comment: true }));
    });
    await waitFor(() => expect(screen.getByRole("switch", { name: "Comments" })).toBeChecked());
  });
});
