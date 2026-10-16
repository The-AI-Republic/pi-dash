// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { PersonalGroupState } from "../api/preferences.js";
import { PROJECT_CAP_MIN_ERROR, PersonalizeDialog } from "./PersonalizeDialog.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";

type PrefsMap = Record<string, { is_pinned: boolean; sort_order: number }>;

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
  propsGetStatus?: number;
  hangPropsGet?: boolean;
  propsPatchStatus?: number;
  prefsPatchStatus?: number;
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
    if (url.endsWith("/sidebar-preferences/") && init.method === "PATCH") {
      const status = options.prefsPatchStatus ?? 200;
      if (status !== 200) {
        return { status, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ message: "ok" }),
      };
    }
    if (url.endsWith("/sidebar-preferences/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ drafts: { is_pinned: true, sort_order: 0 } } satisfies PrefsMap),
      };
    }
    if (init.method === "PATCH") {
      const status = options.propsPatchStatus ?? 200;
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
    if (options.hangPropsGet === true) return new Promise<never>(() => undefined);
    const getStatus = options.propsGetStatus ?? 200;
    if (getStatus !== 200) {
      return { status: getStatus, url, headers: { get: () => null }, text: async () => "{}" };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(propsPayload(serverMode, serverLimit)),
    };
  }) as never);
  return { client, calls };
}

const DRAFTS_READY: PersonalGroupState = {
  status: "ready",
  entries: [{ key: "drafts", label: "Drafts", path: "drafts", enabled: true, sortOrder: 0 }],
};

function renderDialog(options: StubOptions = {}, personal: PersonalGroupState = DRAFTS_READY) {
  const { calls } = stubClient(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onOpenChange = vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <PersonalizeDialog open onOpenChange={onOpenChange} workspaceSlug="acme" personal={personal} />
    </QueryClientProvider>
  );
  const dialog = () => screen.getByRole("dialog", { name: "Personalize sidebar" });
  return { calls, dialog, onOpenChange, unmount: view.unmount };
}

function propsPatches(calls: Array<{ url: string; method: string; body: string }>) {
  return calls.filter((call) => call.method === "PATCH" && call.url.endsWith("/user-properties/"));
}

function prefsPatches(calls: Array<{ url: string; method: string; body: string }>) {
  return calls.filter((call) => call.method === "PATCH" && call.url.endsWith("/sidebar-preferences/"));
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("PersonalizeDialog personal group", () => {
  it("renders the resolved entries with their visibility", () => {
    renderDialog({}, DRAFTS_READY);
    expect(screen.getByRole("checkbox", { name: "Drafts" })).toBeChecked();
  });

  it("toggles an entry with a pin triple", async () => {
    const user = userEvent.setup();
    const h = renderDialog();
    await user.click(screen.getByRole("checkbox", { name: "Drafts" }));
    await waitFor(() => expect(prefsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(prefsPatches(h.calls)[0]!.body)).toEqual([{ key: "drafts", is_pinned: false }]);
  });

  it("persists drops as order triples and ignores self-drops", async () => {
    const personal: PersonalGroupState = {
      status: "ready",
      entries: [
        { key: "a", label: "Alpha", path: "a", enabled: true, sortOrder: 0 },
        { key: "b", label: "Beta", path: "b", enabled: true, sortOrder: 1 },
      ],
    };
    const h = renderDialog({}, personal);
    const rows = screen.getAllByTestId("personal-row");
    fireEvent.dragStart(rows[0]!);
    fireEvent.drop(rows[1]!);
    await waitFor(() => expect(prefsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(prefsPatches(h.calls)[0]!.body)).toEqual([
      { key: "b", sort_order: 0 },
      { key: "a", sort_order: 1 },
    ]);
    fireEvent.dragStart(rows[0]!);
    fireEvent.drop(rows[0]!);
    await waitFor(() => expect(prefsPatches(h.calls)).toHaveLength(1));
  });

  it("flags a failed toggle and rolls back", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ prefsPatchStatus: 500 });
    await user.click(screen.getByRole("checkbox", { name: "Drafts" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't save sidebar preferences", expect.any(String))
    );
    expect(prefsPatches(h.calls)).toHaveLength(1);
  });

  it("skeletons while pending and alerts on error", () => {
    const pending = renderDialog({}, { status: "pending", entries: [] });
    expect(within(pending.dialog()).queryByRole("checkbox", { name: "Drafts" })).toBeNull();
    pending.unmount();
    renderDialog({}, { status: "error", entries: [] });
    expect(screen.getByRole("alert")).toHaveTextContent("Couldn't load personal entries.");
  });
});

describe("PersonalizeDialog projects group", () => {
  it("skeletons while pending and alerts on error", async () => {
    const hanging = renderDialog({ hangPropsGet: true });
    expect(screen.getByRole("heading", { name: "Personalize sidebar" })).not.toBeNull();
    expect(screen.queryByTestId("project-cap-toggle")).toBeNull();
    hanging.unmount();
    renderDialog({ propsGetStatus: 500 });
    await waitFor(() => expect(screen.getByText("Couldn't load display preferences.")).not.toBeNull());
  });

  it("flips the rendering mode with a mode-only write", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ mode: "ACCORDION" });
    const tabbed = await screen.findByTestId("project-nav-tabbed");
    expect(screen.getByTestId("project-nav-accordion")).toBeChecked();
    await user.click(tabbed);
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(propsPatches(h.calls)[0]!.body)).toEqual({ navigation_control_preference: "TABBED" });
    await waitFor(() => expect(screen.getByTestId("project-nav-tabbed")).toBeChecked());
    // Flipping back exercises the accordion radio's own write path.
    await user.click(screen.getByTestId("project-nav-accordion"));
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(2));
    expect(JSON.parse(propsPatches(h.calls)[1]!.body)).toEqual({ navigation_control_preference: "ACCORDION" });
    await waitFor(() => expect(screen.getByTestId("project-nav-accordion")).toBeChecked());
  });

  it("enables the cap with a default count of one", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ limit: 0 });
    const toggle = await screen.findByTestId("project-cap-toggle");
    expect(toggle).not.toBeChecked();
    expect(screen.queryByTestId("project-cap-input")).toBeNull();
    await user.click(toggle);
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(propsPatches(h.calls)[0]!.body)).toEqual({ navigation_project_limit: 1 });
    expect(await screen.findByTestId("project-cap-input")).toHaveValue("1");
  });

  it("remembers the last count across a disable cycle", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ limit: 5 });
    const toggle = await screen.findByTestId("project-cap-toggle");
    expect(toggle).toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(propsPatches(h.calls)[0]!.body)).toEqual({ navigation_project_limit: 0 });
    await user.click(await screen.findByTestId("project-cap-toggle"));
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(2));
    expect(JSON.parse(propsPatches(h.calls)[1]!.body)).toEqual({ navigation_project_limit: 5 });
  });

  it("sends digit counts and strips the rest", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ limit: 2 });
    const input = (await screen.findByTestId("project-cap-input")) as HTMLInputElement;
    expect(input).toHaveValue("2");
    await user.clear(input);
    await user.type(input, "25");
    await waitFor(() => expect(propsPatches(h.calls).length).toBeGreaterThan(0));
    const last = propsPatches(h.calls)[propsPatches(h.calls).length - 1]!;
    expect(JSON.parse(last.body)).toEqual({ navigation_project_limit: 25 });
    await waitFor(() => expect(screen.getByTestId("project-cap-input")).toHaveValue("25"));
  });

  it("blocks exponent keys while digits and edits flow", () => {
    renderDialog({ limit: 2 });
    return screen.findByTestId("project-cap-input").then((input) => {
      expect(fireEvent.keyDown(input, { key: "e" })).toBe(false);
      expect(fireEvent.keyDown(input, { key: "5" })).toBe(true);
      expect(fireEvent.keyDown(input, { key: "Backspace" })).toBe(true);
      expect(fireEvent.keyDown(input, { key: "v", ctrlKey: true })).toBe(true);
      expect(fireEvent.keyDown(input, { key: "v", metaKey: true })).toBe(true);
    });
  });

  it("never lets a typed exponent reach the field", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ limit: 2 });
    const input = (await screen.findByTestId("project-cap-input")) as HTMLInputElement;
    await user.click(input);
    await user.keyboard("{End}");
    await user.type(input, "e");
    expect(input).toHaveValue("2");
    expect(propsPatches(h.calls)).toHaveLength(0);
  });

  it("flags zero inline while the write clamps to one", async () => {
    const h = renderDialog({ limit: 2 });
    const input = await screen.findByTestId("project-cap-input");
    fireEvent.change(input, { target: { value: "0" } });
    expect(await screen.findByText(PROJECT_CAP_MIN_ERROR)).not.toBeNull();
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(propsPatches(h.calls)[0]!.body)).toEqual({ navigation_project_limit: 1 });
    // The clamped draft stays visible with its flag until edited.
    expect(screen.getByTestId("project-cap-input")).toHaveValue("0");
  });

  it("flags an emptied field without writing", async () => {
    const user = userEvent.setup();
    const h = renderDialog({ limit: 2 });
    const input = await screen.findByTestId("project-cap-input");
    await user.clear(input);
    expect(await screen.findByText(PROJECT_CAP_MIN_ERROR)).not.toBeNull();
    expect(propsPatches(h.calls)).toHaveLength(0);
  });

  it("strips pasted non-digits down to the count", async () => {
    const h = renderDialog({ limit: 2 });
    const input = await screen.findByTestId("project-cap-input");
    fireEvent.change(input, { target: { value: "1a2" } });
    await waitFor(() => expect(propsPatches(h.calls)).toHaveLength(1));
    expect(JSON.parse(propsPatches(h.calls)[0]!.body)).toEqual({ navigation_project_limit: 12 });
  });
});
