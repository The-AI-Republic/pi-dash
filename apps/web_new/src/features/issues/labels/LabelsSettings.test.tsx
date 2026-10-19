// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Label } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { MENU_PORTAL_ID } from "../../../shared/shell/document.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import {
  LABEL_DELETE_FAILED_TITLE,
  LABEL_DUPLICATE_MESSAGE,
  LABELS_LOAD_FAILED_TITLE,
  LABEL_UPDATE_FAILED_TITLE,
} from "./api.js";
import { LabelsSettings } from "./LabelsSettings.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const WORKSPACE = "323e4567-e89b-12d3-a456-426614174002";
const AID = "123e4567-e89b-12d3-a456-42661417400a";
const BID = "123e4567-e89b-12d3-a456-42661417400b";
const CID = "123e4567-e89b-12d3-a456-42661417400c";
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

interface LabelsStubOptions {
  labels?: Label[];
  labelsStatus?: number;
  hangLabels?: boolean;
  roles?: Record<string, number>;
  rolesStatus?: number;
  hangRoles?: boolean;
  memberRole?: number;
  memberStatus?: number;
  hangMember?: boolean;
  post?: { status: number; body: unknown };
  patch?: { status: number; body: unknown };
  deleteStatus?: number;
}

function stubLabelsTransport(options: LabelsStubOptions = {}) {
  const serverRows = (options.labels ?? []).map((row) => ({ ...row }));
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => (typeof body === "string" ? body : JSON.stringify(body)),
    });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return json(200, { csrf_token: "csrf" });
    }
    if (url.includes("/project-roles/")) {
      if (options.hangRoles) return new Promise<never>(() => undefined);
      const status = options.rolesStatus ?? 200;
      return json(status, status === 200 ? (options.roles ?? { [PROJECT]: 20 }) : {});
    }
    if (url.includes("/workspace-members/me/")) {
      if (options.hangMember) return new Promise<never>(() => undefined);
      const status = options.memberStatus ?? 200;
      return json(status, status === 200 ? membershipPayload(options.memberRole ?? 20) : {});
    }
    if (url.includes(`/api/workspaces/${SLUG}/projects/${PROJECT}/issue-labels/`)) {
      const method = init.method ?? "GET";
      if (method === "GET") {
        if (options.hangLabels) return new Promise<never>(() => undefined);
        const status = options.labelsStatus ?? 200;
        return json(status, status === 200 ? serverRows : {});
      }
      if (method === "POST") {
        const override = options.post;
        if (override) return json(override.status, override.body);
        const sent = JSON.parse(init.body ?? "{}") as { name: string; color: string };
        const created = { ...labelRow(NEWID, sent.name, 99999), color: sent.color };
        serverRows.push(created);
        return json(201, created);
      }
      const id = url.split("/").filter(Boolean).pop() ?? "";
      if (method === "PATCH") {
        const override = options.patch;
        if (override) return json(override.status, override.body);
        const row = serverRows.find((candidate) => candidate.id === id);
        if (!row) return json(404, {});
        Object.assign(row, JSON.parse(init.body ?? "{}") as Record<string, unknown>);
        return json(200, row);
      }
      if (method === "DELETE") {
        const status = options.deleteStatus ?? 204;
        if (status !== 204) return json(status, {});
        const index = serverRows.findIndex((candidate) => candidate.id === id);
        if (index >= 0) serverRows.splice(index, 1);
        return { status: 204, url, headers: { get: () => null }, text: async () => "" };
      }
    }
    throw new Error(`unexpected request ${init.method ?? "GET"} ${url}`);
  }) as never);
  return { calls, serverRows };
}

function setup(options: LabelsStubOptions = {}) {
  const stub = stubLabelsTransport(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <LabelsSettings workspaceSlug={SLUG} projectId={PROJECT} />
    </QueryClientProvider>
  );
  return { ...stub, queryClient };
}

/** Names in render order across groups and children. */
function visibleNames(): string[] {
  return screen
    .getAllByTestId("label-row-name")
    .filter((name) => name.closest('[data-testid="label-drag-chip"]') === null)
    .map((name) => name.textContent ?? "");
}

function rowByName(name: string): HTMLElement {
  for (const span of screen.getAllByTestId("label-row-name")) {
    const row = span.closest('[data-testid="label-row"]');
    if (span.textContent === name && row instanceof HTMLElement) return row;
  }
  throw new Error(`no row for ${name}`);
}

function stubSlotRects(spans: Record<string, { top: number; bottom: number }>): void {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const rect = spans[this.dataset["labelId"] ?? ""];
    const top = rect?.top ?? 0;
    const bottom = rect?.bottom ?? 0;
    return {
      x: 0,
      y: top,
      top,
      bottom,
      left: 0,
      right: 400,
      width: 400,
      height: bottom - top,
      toJSON: () => ({}),
    } as DOMRect;
  });
}

function pointerMove(clientX: number, clientY: number): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointermove", { clientX, clientY, bubbles: true }));
  });
}

function pointerUp(): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointerup", { bubbles: true }));
  });
}

function pointerCancel(): void {
  act(() => {
    window.dispatchEvent(new window.MouseEvent("pointercancel", { bubbles: true }));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  const host = document.createElement("div");
  host.id = MENU_PORTAL_ID;
  document.body.appendChild(host);
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  document.getElementById(MENU_PORTAL_ID)?.remove();
});

describe("LabelsSettings standing", () => {
  it("shows the skeleton while loading", () => {
    setup({ hangLabels: true, hangRoles: true, hangMember: true });
    expect(screen.getByRole("heading", { name: "Labels" })).toBeInTheDocument();
    expect(screen.getByTestId("labels-skeleton")).toHaveAttribute("role", "status");
    expect(screen.queryByTestId("label-add")).not.toBeInTheDocument();
  });

  it("denies guests with the not-authorized view", async () => {
    setup({ roles: { [PROJECT]: 5 }, memberRole: 5, labels: [labelRow(AID, "Alpha", 10000)] });
    await waitFor(() => expect(screen.getByRole("heading", { name: /not authorized/i })).toBeInTheDocument());
    expect(screen.queryByTestId("label-row")).not.toBeInTheDocument();
  });

  it("admits workspace admins to edit", async () => {
    setup({ roles: {}, memberRole: 20, labels: [] });
    await waitFor(() => expect(screen.getByTestId("label-add")).toBeInTheDocument());
    expect(screen.getByText("No labels yet")).toBeInTheDocument();
  });

  it("reads the list without editing when standing fails", async () => {
    setup({
      rolesStatus: 500,
      memberStatus: 500,
      labels: [labelRow(AID, "Alpha", 10000)],
    });
    await waitFor(() => expect(screen.getByText("Alpha")).toBeInTheDocument());
    expect(screen.queryByTestId("label-add")).not.toBeInTheDocument();
    expect(screen.queryByTestId("label-drag-handle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("label-row-menu")).not.toBeInTheDocument();
  });

  it("notices a failed list load and renders empty", async () => {
    setup({ labelsStatus: 500 });
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(LABELS_LOAD_FAILED_TITLE));
    expect(screen.getByText("No labels yet")).toBeInTheDocument();
  });
});

describe("LabelsSettings empty state", () => {
  it("offers creation to admins", async () => {
    const user = userEvent.setup();
    setup({ labels: [] });
    await waitFor(() => expect(screen.getByText("No labels yet")).toBeInTheDocument());
    expect(screen.getByTestId("label-add")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create your first label" }));
    expect(screen.getByTestId("label-form")).toBeInTheDocument();
    expect(screen.queryByText("No labels yet")).not.toBeInTheDocument();
  });

  it("hides the create action from members", async () => {
    setup({ roles: { [PROJECT]: 15 }, memberRole: 15, labels: [] });
    await waitFor(() => expect(screen.getByText("No labels yet")).toBeInTheDocument());
    expect(screen.queryByTestId("label-add")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create your first label" })).not.toBeInTheDocument();
  });

  it("suppresses the skeleton while the create form is open mid-load", async () => {
    const user = userEvent.setup();
    setup({ hangLabels: true });
    await waitFor(() => expect(screen.getByTestId("label-add")).toBeInTheDocument());
    expect(screen.getByTestId("labels-skeleton")).toBeInTheDocument();
    await user.click(screen.getByTestId("label-add"));
    expect(screen.getByTestId("label-form")).toBeInTheDocument();
    expect(screen.queryByTestId("labels-skeleton")).not.toBeInTheDocument();
  });
});

describe("LabelsSettings groups", () => {
  const grouped = [labelRow(BID, "Beta", 20000), labelRow(AID, "Alpha", 10000), labelRow(KID, "Kid", 12000, AID)];

  it("renders parents with their children in order, collapsing on toggle", async () => {
    const user = userEvent.setup();
    setup({ labels: grouped });
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Kid", "Beta"]));
    expect(rowByName("Kid").querySelector("[data-label-slot]")).toHaveClass("ml-(--space-8)");
    expect(rowByName("Beta").querySelector("[data-label-slot]")).not.toHaveClass("ml-(--space-8)");
    const toggle = screen.getByRole("button", { name: "Collapse Alpha" });
    await user.click(toggle);
    expect(visibleNames()).toEqual(["Alpha", "Beta"]);
    expect(screen.getByRole("button", { name: "Expand Alpha" })).toHaveAttribute("aria-expanded", "false");
    await user.click(screen.getByRole("button", { name: "Expand Alpha" }));
    expect(visibleNames()).toEqual(["Alpha", "Kid", "Beta"]);
  });

  it("shows no editing affordances to members", async () => {
    setup({ roles: { [PROJECT]: 15 }, memberRole: 15, labels: grouped });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    expect(screen.queryByTestId("label-add")).not.toBeInTheDocument();
    expect(screen.queryByTestId("label-drag-handle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("label-row-menu")).not.toBeInTheDocument();
    expect(screen.queryByTestId("label-delete")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse Alpha" })).toBeInTheDocument();
  });
});

describe("LabelsSettings create form", () => {
  it("creates through the header action and closes on success", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: [labelRow(AID, "Alpha", 10000)] });
    await waitFor(() => expect(screen.getByText("Alpha")).toBeInTheDocument());
    await user.click(screen.getByTestId("label-add"));
    expect(screen.queryByTestId("label-add")).not.toBeInTheDocument();
    await user.type(screen.getByTestId("label-form-name"), "Fresh");
    await user.click(screen.getByRole("button", { name: "Create label" }));
    await waitFor(() => expect(screen.getByText("Fresh")).toBeInTheDocument());
    expect(screen.queryByTestId("label-form")).not.toBeInTheDocument();
    const post = calls.find((call) => call.method === "POST");
    expect(JSON.parse(post?.body ?? "{}")).toMatchObject({ name: "Fresh" });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("cancels without posting", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: [] });
    await waitFor(() => expect(screen.getByTestId("label-add")).toBeInTheDocument());
    await user.click(screen.getByTestId("label-add"));
    await user.type(screen.getByTestId("label-form-name"), "Abandoned");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByTestId("label-form")).not.toBeInTheDocument();
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("keeps the form open with a duplicate notice on refusal", async () => {
    const user = userEvent.setup();
    setup({
      labels: [labelRow(AID, "Alpha", 10000)],
      post: { status: 400, body: { name: ["LABEL_NAME_ALREADY_EXISTS"] } },
    });
    await waitFor(() => expect(screen.getByText("Alpha")).toBeInTheDocument());
    await user.click(screen.getByTestId("label-add"));
    await user.type(screen.getByTestId("label-form-name"), "Alpha");
    await user.click(screen.getByRole("button", { name: "Create label" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(expect.anything(), LABEL_DUPLICATE_MESSAGE));
    expect(screen.getByTestId("label-form")).toBeInTheDocument();
    expect(screen.getByTestId("label-form-name")).toHaveValue("Alpha");
  });
});

describe("LabelsSettings row menus", () => {
  const grouped = [labelRow(AID, "Alpha", 10000), labelRow(KID, "Kid", 12000, AID), labelRow(BID, "Beta", 20000)];

  function menuTexts(): string[] {
    return screen
      .getAllByRole("menuitem")
      .map((item) => item.textContent?.trim() ?? "")
      .filter((text) => text !== "");
  }

  it("edits a plain label through its only menu entry", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: grouped });
    await waitFor(() => expect(screen.getByText("Beta")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Beta" }));
    expect(menuTexts()).toEqual(["Edit label"]);
    await user.click(screen.getByRole("menuitem", { name: "Edit label" }));
    expect(screen.getByTestId("label-form-name")).toHaveValue("Beta");
    expect(screen.queryByTestId("label-add")).not.toBeInTheDocument();
    await user.clear(screen.getByTestId("label-form-name"));
    await user.type(screen.getByTestId("label-form-name"), "Gamma");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toMatchObject({ name: "Gamma" });
  });

  it("restores the row when the edit cancels", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: grouped });
    await waitFor(() => expect(screen.getByText("Beta")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Beta" }));
    await user.click(screen.getByRole("menuitem", { name: "Edit label" }));
    await user.type(screen.getByTestId("label-form-name"), " plus more");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByText("Beta")).toBeInTheDocument();
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("rolls a failed rename back with a notice", async () => {
    const user = userEvent.setup();
    setup({ labels: grouped, patch: { status: 404, body: {} } });
    await waitFor(() => expect(screen.getByText("Beta")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Beta" }));
    await user.click(screen.getByRole("menuitem", { name: "Edit label" }));
    await user.clear(screen.getByTestId("label-form-name"));
    await user.type(screen.getByTestId("label-form-name"), "Ghost");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(LABEL_UPDATE_FAILED_TITLE, expect.anything()));
    expect(screen.getByTestId("label-form")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.getByText("Beta")).toBeInTheDocument());
  });

  it("edits a child and a group parent through their rows", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: grouped });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Kid" }));
    expect(menuTexts()).toEqual(["Remove from group", "Edit label"]);
    await user.click(screen.getByRole("menuitem", { name: "Edit label" }));
    expect(screen.getByTestId("label-form-name")).toHaveValue("Kid");
    await user.clear(screen.getByTestId("label-form-name"));
    await user.type(screen.getByTestId("label-form-name"), "Kiddo");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(screen.getByText("Kiddo")).toBeInTheDocument());
    const childPatch = calls.find((call) => call.method === "PATCH");
    expect(childPatch?.url).toContain(KID);
    expect(JSON.parse(childPatch?.body ?? "{}")).toMatchObject({ name: "Kiddo" });
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha" }));
    expect(menuTexts()).toEqual(["Edit label", "Delete label"]);
    await user.click(screen.getByRole("menuitem", { name: "Edit label" }));
    expect(screen.getByTestId("label-form-name")).toHaveValue("Alpha");
    await user.clear(screen.getByTestId("label-form-name"));
    await user.type(screen.getByTestId("label-form-name"), "Alpha Prime");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(screen.getByText("Alpha Prime")).toBeInTheDocument());
  });

  it("detaches a child through its menu without an order write", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: grouped });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for Kid" }));
    await user.click(screen.getByRole("menuitem", { name: "Remove from group" }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH" && call.url.includes(KID))).toBe(true));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ parent: null });
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Kid", "Beta"]));
  });
});

describe("LabelsSettings delete", () => {
  const grouped = [labelRow(AID, "Alpha", 10000), labelRow(BID, "Beta", 20000)];

  it("deletes a row through its trash control", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ labels: grouped });
    await waitFor(() => expect(screen.getByText("Beta")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Delete Beta" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("Beta");
    await user.click(screen.getByRole("button", { name: "Delete label" }));
    await waitFor(() => expect(screen.queryByText("Beta")).not.toBeInTheDocument());
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(BID))).toBe(true);
    expect(screen.getByText("Alpha")).toBeInTheDocument();
  });

  it("deletes a group through its menu", async () => {
    const user = userEvent.setup();
    setup({ labels: [...grouped, labelRow(KID, "Kid", 12000, AID)] });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Delete Alpha" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete label" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("Alpha");
    await user.click(screen.getByRole("button", { name: "Delete label" }));
    await waitFor(() => expect(screen.queryByText("Alpha")).not.toBeInTheDocument());
  });

  it("cancels cleanly and keeps the dialog open on failure", async () => {
    const user = userEvent.setup();
    setup({ labels: grouped, deleteStatus: 500 });
    await waitFor(() => expect(screen.getByText("Beta")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Delete Beta" }));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete Beta" }));
    await user.click(screen.getByRole("button", { name: "Delete label" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(LABEL_DELETE_FAILED_TITLE));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
  });
});

describe("LabelsSettings drag", () => {
  const flat = [labelRow(AID, "Alpha", 10000), labelRow(BID, "Beta", 20000), labelRow(CID, "Gamma", 30000)];

  function gripByName(name: string): HTMLElement {
    const grip = rowByName(name).querySelector('[data-testid="label-drag-handle"]');
    if (!(grip instanceof HTMLElement)) throw new Error(`no grip for ${name}`);
    return grip;
  }

  function ringed(name: string): boolean {
    return rowByName(name).querySelector('[data-drop-into="true"]') !== null;
  }

  async function patchBody(
    calls: Array<{ url: string; method: string; body: string }>
  ): Promise<Record<string, unknown>> {
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    return JSON.parse(calls.find((call) => call.method === "PATCH")?.body ?? "{}") as Record<string, unknown>;
  }

  it("reorders above with the insertion line", async () => {
    const { calls } = setup({ labels: flat });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Gamma"));
    pointerMove(200, 2);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "above");
    expect(line.closest('[data-testid="label-row"]')).toBe(rowByName("Alpha"));
    expect(rowByName("Gamma").querySelector("[data-label-slot]")).toHaveClass("opacity-60");
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: null, sort_order: 5000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Gamma", "Alpha", "Beta"]));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reorders below the last row", async () => {
    const { calls } = setup({ labels: flat });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Alpha"));
    pointerMove(200, 500);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "below");
    expect(line.closest('[data-testid="label-row"]')).toBe(rowByName("Gamma"));
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: null, sort_order: 40000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Beta", "Gamma", "Alpha"]));
  });

  it("nests onto a label with the ring, leaving the order to the server", async () => {
    const { calls } = setup({ labels: flat });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Gamma"));
    pointerMove(200, 20);
    expect(ringed("Alpha")).toBe(true);
    expect(rowByName("Alpha").querySelector('[data-drop-into="true"]')).toHaveClass("ring-2", "ring-(--accent)");
    expect(screen.queryByTestId("label-drop-line")).not.toBeInTheDocument();
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: AID });
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Gamma", "Beta"]));
  });

  it("converts a group drop to reorder-above", async () => {
    const { calls } = setup({ labels: [...flat, labelRow(KID, "Kid", 12000, AID)] });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    stubSlotRects({
      [AID]: { top: 0, bottom: 40 },
      [KID]: { top: 40, bottom: 80 },
      [BID]: { top: 80, bottom: 120 },
      [CID]: { top: 120, bottom: 160 },
    });
    fireEvent.pointerDown(gripByName("Alpha"));
    pointerMove(200, 140);
    expect(ringed("Gamma")).toBe(false);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "above");
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: null, sort_order: 25000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Beta", "Alpha", "Kid", "Gamma"]));
  });

  it("lands onto-child drops as siblings", async () => {
    const { calls } = setup({ labels: [flat[0]!, flat[1]!, labelRow(KID, "Kid", 12000, AID)] });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [KID]: { top: 40, bottom: 80 }, [BID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Beta"));
    pointerMove(200, 60);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "above");
    expect(line.closest('[data-testid="label-row"]')).toBe(rowByName("Kid"));
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: AID, sort_order: 6000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Beta", "Kid"]));
  });

  it("nests onto a group with the ring, appending past its children", async () => {
    const { calls } = setup({ labels: [flat[0]!, flat[1]!, labelRow(KID, "Kid", 12000, AID)] });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [KID]: { top: 40, bottom: 80 }, [BID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Beta"));
    pointerMove(200, 20);
    expect(ringed("Alpha")).toBe(true);
    expect(rowByName("Alpha").querySelector('[data-drop-into="true"]')).toHaveClass("ring-2", "ring-(--accent)");
    expect(screen.queryByTestId("label-drop-line")).not.toBeInTheDocument();
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: AID, sort_order: 22000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Kid", "Beta"]));
  });

  it("reorders above a group row with the insertion line", async () => {
    const { calls } = setup({ labels: [flat[0]!, flat[1]!, labelRow(KID, "Kid", 12000, AID)] });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [KID]: { top: 40, bottom: 80 }, [BID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Beta"));
    pointerMove(200, 2);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "above");
    expect(line.closest('[data-testid="label-row"]')).toBe(rowByName("Alpha"));
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: null, sort_order: 5000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Beta", "Alpha", "Kid"]));
  });

  it("reorders below a collapsed group", async () => {
    const { calls } = setup({
      labels: [labelRow(BID, "Beta", 10000), labelRow(AID, "Alpha", 20000), labelRow(KID, "Kid", 12000, AID)],
    });
    await waitFor(() => expect(visibleNames()).toEqual(["Beta", "Alpha", "Kid"]));
    fireEvent.click(screen.getByRole("button", { name: "Collapse Alpha" }));
    expect(visibleNames()).toEqual(["Beta", "Alpha"]);
    stubSlotRects({ [BID]: { top: 0, bottom: 40 }, [AID]: { top: 40, bottom: 80 } });
    fireEvent.pointerDown(gripByName("Beta"));
    pointerMove(200, 500);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "below");
    expect(line.closest('[data-testid="label-row"]')).toBe(rowByName("Alpha"));
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: null, sort_order: 30000 });
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Beta"]));
  });

  it("follows the pointer with a preview chip while dragging", async () => {
    setup({ labels: flat });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Gamma"));
    expect(screen.queryByTestId("label-drag-chip")).not.toBeInTheDocument();
    pointerMove(200, 50);
    const chip = screen.getByTestId("label-drag-chip");
    expect(chip).toHaveTextContent("Gamma");
    expect(chip).toHaveAttribute("aria-hidden", "true");
    expect(chip.style.transform).toBe("translate3d(212px, 62px, 0)");
    pointerMove(310, 90);
    expect(screen.getByTestId("label-drag-chip").style.transform).toBe("translate3d(322px, 102px, 0)");
    pointerUp();
    expect(screen.queryByTestId("label-drag-chip")).not.toBeInTheDocument();
  });

  it("drops a top-level row below the last child's lower half", async () => {
    const { calls } = setup({ labels: [flat[0]!, flat[1]!, labelRow(KID, "Kid", 12000, AID)] });
    await waitFor(() => expect(screen.getByText("Kid")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [KID]: { top: 40, bottom: 80 }, [BID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Beta"));
    pointerMove(200, 75);
    const line = screen.getByTestId("label-drop-line");
    expect(line).toHaveAttribute("data-position", "below");
    expect(line.closest('[data-testid="label-row"]')).toBe(rowByName("Kid"));
    pointerUp();
    expect(await patchBody(calls)).toEqual({ parent: AID, sort_order: 22000 });
  });

  it("refuses self drops silently", async () => {
    const { calls } = setup({ labels: flat });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Beta"));
    pointerMove(200, 50);
    expect(screen.queryByTestId("label-drop-line")).not.toBeInTheDocument();
    pointerUp();
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Beta", "Gamma"]));
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("drops nothing on pointer cancel", async () => {
    const { calls } = setup({ labels: flat });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Gamma"));
    pointerMove(200, 2);
    expect(screen.getByTestId("label-drop-line")).toBeInTheDocument();
    pointerCancel();
    expect(screen.queryByTestId("label-drop-line")).not.toBeInTheDocument();
    expect(calls.some((call) => call.method === "PATCH")).toBe(false);
  });

  it("rolls a failed drop back with no notice", async () => {
    const { calls, serverRows } = setup({ labels: flat, patch: { status: 500, body: {} } });
    await waitFor(() => expect(screen.getByText("Gamma")).toBeInTheDocument());
    stubSlotRects({ [AID]: { top: 0, bottom: 40 }, [BID]: { top: 40, bottom: 80 }, [CID]: { top: 80, bottom: 120 } });
    fireEvent.pointerDown(gripByName("Gamma"));
    pointerMove(200, 2);
    pointerUp();
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    await waitFor(() => expect(visibleNames()).toEqual(["Alpha", "Beta", "Gamma"]));
    expect(serverRows.find((row) => row.id === CID)?.sort_order).toBe(30000);
    expect(notifyError).not.toHaveBeenCalled();
  });
});
