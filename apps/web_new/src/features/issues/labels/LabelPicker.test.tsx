// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Label } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import { LABEL_CREATE_FAILED_TITLE, LABEL_DUPLICATE_MESSAGE } from "./api.js";
import { LabelPicker } from "./LabelPicker.js";
import { LABEL_PALETTE } from "./palette.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const WORKSPACE = "323e4567-e89b-12d3-a456-426614174002";
const AID = "123e4567-e89b-12d3-a456-42661417400a";
const BID = "123e4567-e89b-12d3-a456-42661417400b";
const KID = "123e4567-e89b-12d3-a456-42661417400d";
const NEWID = "123e4567-e89b-12d3-a456-42661417400e";

function labelRow(id: string, name: string, sort: number, parent: string | null = null): Label {
  return { id, project_id: PROJECT, workspace_id: WORKSPACE, name, color: "#FF6900", parent, sort_order: sort };
}

function membershipPayload(role: number): Record<string, unknown> {
  return {
    id: "423e4567-e89b-12d3-a456-426614174000",
    workspace: WORKSPACE,
    member: "523e4567-e89b-12d3-a456-426614174000",
    role,
    company_role: null,
    view_props: {},
    default_props: {},
    issue_props: {},
    is_active: true,
    getting_started_checklist: {},
    tips: {},
    explored_features: {},
    draft_issue_count: 0,
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

interface PickerStubOptions {
  labels?: Label[];
  hangLabels?: boolean;
  roles?: Record<string, number>;
  rolesStatus?: number;
  memberRole?: number;
  memberStatus?: number;
  post?: { status: number; body: unknown };
}

function stubPickerTransport(options: PickerStubOptions = {}) {
  const serverRows = (options.labels ?? []).map((row) => ({ ...row }));
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return json(200, { csrf_token: "csrf" });
    }
    if (url.includes("/project-roles/")) {
      const status = options.rolesStatus ?? 200;
      return json(status, status === 200 ? (options.roles ?? { [PROJECT]: 20 }) : {});
    }
    if (url.includes("/workspace-members/me/")) {
      const status = options.memberStatus ?? 200;
      return json(status, status === 200 ? membershipPayload(options.memberRole ?? 15) : {});
    }
    if (url.includes(`/api/workspaces/${SLUG}/projects/${PROJECT}/issue-labels/`)) {
      const method = init.method ?? "GET";
      if (method === "GET") {
        if (options.hangLabels) return new Promise<never>(() => undefined);
        return json(200, serverRows);
      }
      if (method === "POST") {
        const override = options.post;
        if (override) return json(override.status, override.body);
        const sent = JSON.parse(init.body ?? "{}") as { name: string; color: string };
        const created = { ...labelRow(NEWID, sent.name, 99999), color: sent.color };
        serverRows.push(created);
        return json(201, created);
      }
    }
    throw new Error(`unexpected request ${init.method ?? "GET"} ${url}`);
  }) as never);
  return { calls, serverRows };
}

const ROWS = [labelRow(AID, "Alpha", 10000), labelRow(KID, "Kid", 12000, AID), labelRow(BID, "Beta", 20000)];

function setup(options: PickerStubOptions = {}, props?: Partial<React.ComponentProps<typeof LabelPicker>>) {
  const stub = stubPickerTransport({ labels: ROWS, ...options });
  const onChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <LabelPicker workspaceSlug={SLUG} projectId={PROJECT} value={[]} onChange={onChange} {...props} />
    </QueryClientProvider>
  );
  return { ...stub, onChange };
}

function optionTexts(): string[] {
  return screen.getAllByTestId("label-picker-option").map((option) => option.textContent ?? "");
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("LabelPicker options", () => {
  it("fetches lazily and lists grouped options with selection pressed", async () => {
    const user = userEvent.setup();
    const { calls } = setup({}, { value: [KID] });
    expect(calls.some((call) => call.url.includes("/issue-labels/"))).toBe(false);
    expect(screen.getByTestId("label-picker-trigger")).toHaveTextContent("1 selected");
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toEqual(["Alpha", "Kid", "Beta"]));
    const options = screen.getAllByTestId("label-picker-option");
    expect(options[0]).toHaveAttribute("aria-pressed", "false");
    expect(options[1]).toHaveAttribute("aria-pressed", "true");
    expect(options[1]).toHaveClass("pl-(--space-8)");
    expect(options[0]).not.toHaveClass("pl-(--space-8)");
  });

  it("toggles selection without closing", async () => {
    const user = userEvent.setup();
    const { onChange } = setup({}, { value: [AID] });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toEqual(["Alpha", "Kid", "Beta"]));
    await user.click(screen.getAllByTestId("label-picker-option")[1]!);
    expect(onChange).toHaveBeenCalledWith([AID, KID]);
    await user.click(screen.getAllByTestId("label-picker-option")[0]!);
    expect(onChange).toHaveBeenCalledWith([]);
    expect(screen.getByTestId("label-picker-search")).toBeInTheDocument();
  });

  it("filters on search with an autofocused field", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    expect(screen.getByTestId("label-picker-search")).toHaveFocus();
    await user.type(screen.getByTestId("label-picker-search"), "bet");
    expect(optionTexts()).toEqual(["Beta"]);
  });

  it("clears the query on first Escape and closes on the next", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "bet");
    expect(optionTexts()).toEqual(["Beta"]);
    await user.keyboard("{Escape}");
    expect(screen.getByTestId("label-picker-search")).toHaveValue("");
    expect(optionTexts()).toEqual(["Alpha", "Kid", "Beta"]);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("label-picker-search")).not.toBeInTheDocument());
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(screen.getByTestId("label-picker-search")).toHaveValue(""));
  });

  it("shows the loading and empty notes", async () => {
    const user = userEvent.setup();
    setup({ hangLabels: true, labels: [] });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(screen.getByTestId("label-picker-loading")).toBeInTheDocument());
  });

  it("shows the empty-collection note", async () => {
    const user = userEvent.setup();
    setup({ labels: [] });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(screen.getByTestId("label-picker-empty")).toHaveTextContent("No labels yet"));
  });
});

describe("LabelPicker keyboard submit", () => {
  it("selects the exact match instead of creating", async () => {
    const user = userEvent.setup();
    const { calls, onChange } = setup();
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "beta");
    await user.keyboard("{Enter}");
    expect(onChange).toHaveBeenCalledWith([BID]);
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("selects the first option for a partial match", async () => {
    const user = userEvent.setup();
    const { onChange } = setup();
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "a");
    await user.keyboard("{Enter}");
    expect(onChange).toHaveBeenCalledWith([AID]);
  });

  it("ignores Enter on a blank query", async () => {
    const user = userEvent.setup();
    const { calls, onChange } = setup();
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.keyboard("{Enter}");
    expect(onChange).not.toHaveBeenCalled();
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });
});

// Base UI mounts the popover in a follow-up task and the option list
// renders after that; under a full-suite run the pair can outlast the
// 1 s default wait, so the settled list gets an explicit budget.
const POPOVER_TIMEOUT = { timeout: 10_000 };

describe("LabelPicker inline create", () => {
  it("creates and selects a fresh name for admins", async () => {
    const user = userEvent.setup();
    const { calls, onChange } = setup({ roles: { [PROJECT]: 20 }, memberRole: 15 });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "Fresh");
    await waitFor(() => expect(screen.getByTestId("label-picker-create")).toHaveTextContent('Create "Fresh"'));
    await user.click(screen.getByTestId("label-picker-create"));
    await waitFor(() => expect(onChange).toHaveBeenCalledWith([NEWID]));
    const post = calls.find((call) => call.method === "POST");
    const sent = JSON.parse(post?.body ?? "{}") as { name: string; color: string };
    expect(sent.name).toBe("Fresh");
    expect([...LABEL_PALETTE]).toContain(sent.color);
    expect(screen.getByTestId("label-picker-search")).toHaveValue("");
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("creates through Enter and keeps the popover open", async () => {
    const user = userEvent.setup();
    const { onChange } = setup({ roles: { [PROJECT]: 15 }, memberRole: 20 });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "Fresh");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onChange).toHaveBeenCalledWith([NEWID]));
    expect(screen.getByTestId("label-picker-search")).toBeInTheDocument();
  });

  it("notices duplicates and keeps the query", async () => {
    const user = userEvent.setup();
    const { onChange } = setup({ post: { status: 400, body: { name: ["LABEL_NAME_ALREADY_EXISTS"] } } });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "Fresh");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(expect.anything(), LABEL_DUPLICATE_MESSAGE));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("label-picker-search")).toHaveValue("Fresh");
  });

  it("refuses over-long names without posting", async () => {
    const user = userEvent.setup();
    const { calls } = setup();
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "x".repeat(256));
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(LABEL_CREATE_FAILED_TITLE, expect.stringContaining("255"))
    );
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  }, 30_000);

  it("offers non-admins the empty note instead of creation", async () => {
    const user = userEvent.setup();
    const { onChange } = setup({ roles: { [PROJECT]: 15 }, memberRole: 15 });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "zzz");
    await waitFor(
      () => expect(screen.getByTestId("label-picker-empty")).toHaveTextContent("No matching results"),
      POPOVER_TIMEOUT
    );
    // The empty note landed after the picker was interactive.
    expect(screen.getByTestId("label-picker-search")).toHaveFocus();
    expect(screen.queryByTestId("label-picker-create")).not.toBeInTheDocument();
    await user.keyboard("{Enter}");
    expect(onChange).not.toHaveBeenCalled();
  }, 30_000);

  it("hides creation when standing fails", async () => {
    const user = userEvent.setup();
    setup({ rolesStatus: 500 });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "zzz");
    await waitFor(
      () => expect(screen.getByTestId("label-picker-empty")).toHaveTextContent("No matching results"),
      POPOVER_TIMEOUT
    );
    expect(screen.queryByTestId("label-picker-create")).not.toBeInTheDocument();
  }, 30_000);

  it("hides creation when the membership read fails", async () => {
    const user = userEvent.setup();
    setup({ memberStatus: 500 });
    await user.click(screen.getByTestId("label-picker-trigger"));
    await waitFor(() => expect(optionTexts()).toHaveLength(3), POPOVER_TIMEOUT);
    await user.type(screen.getByTestId("label-picker-search"), "zzz");
    await waitFor(
      () => expect(screen.getByTestId("label-picker-empty")).toHaveTextContent("No matching results"),
      POPOVER_TIMEOUT
    );
    expect(screen.queryByTestId("label-picker-create")).not.toBeInTheDocument();
  }, 30_000);
});

describe("LabelPicker chrome", () => {
  it("locks the trigger when disabled", () => {
    setup({}, { disabled: true });
    expect(screen.getByTestId("label-picker-trigger")).toBeDisabled();
    expect(screen.getByTestId("label-picker-trigger")).toHaveTextContent("Select labels");
  });

  it("names the trigger from the label prop", () => {
    setup({}, { label: "Sidebar labels" });
    expect(screen.getByTestId("label-picker-trigger")).toHaveAttribute("aria-label", "Sidebar labels");
  });
});
