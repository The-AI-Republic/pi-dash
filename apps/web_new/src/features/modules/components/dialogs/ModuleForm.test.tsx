// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module write dialog in both modes (parity/specs/modules.md §7.1,
// §7.2, MOD-022–024): create opens blank with the restricted project
// picker, update opens prefilled without it; the pinned keyboard sequence
// with the date control third (NEWFRONT-330); title validation; Escape and
// Cancel close without writing; success closes with a notice and refreshes
// the list reads; a failure keeps the dialog open with every field intact
// (NEWFRONT-328) and sends the server's own sentence to the error toast.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/shell/index.js")>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { moduleKeys } from "../../api/keys.js";
import { MODULES_MODULE_ID, MODULES_PROJECT_ID, MODULES_WS_ID, makeModuleRow } from "../../testing.js";
import { ModuleSaveDialog } from "./ModuleSaveDialog.js";

const PROJECT_B_ID = "523e4567-e89b-12d3-a456-426614174004";
const ME_ID = "423e4567-e89b-12d3-a456-426614174003";
const TEAMMATE_ID = "623e4567-e89b-12d3-a456-426614174005";

interface Call {
  method: string;
  url: string;
  body: Record<string, unknown> | null;
}

interface Harness {
  projectsStatus: number;
  createStatus: number;
  createBody: unknown;
  updateStatus: number;
  writes: Call[];
  modulesReads: number;
  blockWrite: Promise<void> | null;
}

let harness: Harness;

function json(url: string, status: number, body: unknown) {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
}

function projectRow(id: string, name: string, identifier: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: MODULES_WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function memberRow(id: string, display: string, membershipId: string) {
  return {
    id: membershipId,
    role: 15,
    member: id,
    project: MODULES_PROJECT_ID,
    original_role: 15,
    created_at: "2026-01-02T03:04:05Z",
  };
}

function directoryMember(id: string, first: string, last: string, membershipId: string) {
  return {
    id: membershipId,
    role: 15,
    member: {
      id,
      first_name: first,
      last_name: last,
      avatar: "",
      avatar_url: null,
      display_name: `${first} ${last}`,
      is_bot: false,
    },
  };
}

const ME = {
  id: ME_ID,
  email: "rich@test.dev",
  username: "rich",
  first_name: "Rich",
  last_name: "Owner",
  display_name: "Rich Owner",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-02T03:04:05Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

beforeEach(() => {
  harness = {
    projectsStatus: 200,
    createStatus: 201,
    createBody: null,
    updateStatus: 200,
    writes: [],
    modulesReads: 0,
    blockWrite: null,
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, 200, { csrf_token: "csrf" });
    const method = init.method ?? "GET";
    const body = init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>);
    if (url === "https://api.test/api/workspaces/acme/projects/") {
      return json(url, harness.projectsStatus, [
        projectRow(MODULES_PROJECT_ID, "Website relaunch", "WEB"),
        projectRow(PROJECT_B_ID, "Project B", "PB"),
        projectRow("623e4567-e89b-12d3-a456-426614174006", "Guest Only", "GO", { member_role: 5 }),
        projectRow("723e4567-e89b-12d3-a456-426614174007", "Modules Off", "MO", { module_view: false }),
        projectRow("823e4567-e89b-12d3-a456-426614174008", "Archived", "AR", {
          archived_at: "2026-01-02T03:04:05Z",
        }),
      ]);
    }
    if (url.endsWith("/api/users/me/")) return json(url, 200, ME);
    if (url.endsWith("/members/") && url.includes("/projects/")) {
      return json(url, 200, [
        memberRow(ME_ID, "Rich Owner", "123e4567-e89b-12d3-a456-426614174201"),
        memberRow(TEAMMATE_ID, "Pat Member", "123e4567-e89b-12d3-a456-426614174202"),
      ]);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) {
      return json(url, 200, [
        directoryMember(ME_ID, "Rich", "Owner", "123e4567-e89b-12d3-a456-426614174203"),
        directoryMember(TEAMMATE_ID, "Pat", "Member", "123e4567-e89b-12d3-a456-426614174204"),
      ]);
    }
    if (url.endsWith(`/projects/${MODULES_PROJECT_ID}/modules/`) && method === "GET") {
      harness.modulesReads += 1;
      return json(url, 200, [makeModuleRow()]);
    }
    if (url.endsWith(`/projects/${MODULES_PROJECT_ID}/archived-modules/`) && method === "GET") {
      return json(url, 200, []);
    }
    if (url.endsWith("/modules/") && method === "POST") {
      const call: Call = { method, url, body };
      harness.writes.push(call);
      if (harness.blockWrite !== null) await harness.blockWrite;
      if (harness.createStatus >= 400) {
        return json(url, harness.createStatus, { error: "Module with this name already exists" });
      }
      const echo = harness.createBody ?? makeModuleRow({ id: PROJECT_B_ID, name: body?.["name"] as string });
      return json(url, harness.createStatus, echo);
    }
    if (url.endsWith(`/modules/${MODULES_MODULE_ID}/`) && method === "PATCH") {
      const call: Call = { method, url, body };
      harness.writes.push(call);
      if (harness.blockWrite !== null) await harness.blockWrite;
      if (harness.updateStatus >= 400) return json(url, harness.updateStatus, { error: "Refused." });
      return json(url, 200, {
        id: MODULES_MODULE_ID,
        name: body?.["name"] ?? "Delivery",
        project_id: MODULES_PROJECT_ID,
        description: body?.["description"] ?? "",
        start_date: body?.["start_date"] ?? null,
        target_date: body?.["target_date"] ?? null,
        status: body?.["status"] ?? "backlog",
      });
    }
    throw new Error(`unexpected request: ${method} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockReset();
  notify.notifySuccess.mockReset();
});

const CURRENT_PROJECT = { id: MODULES_PROJECT_ID, name: "Website relaunch", identifier: "WEB" };

function renderDialog(props: Partial<React.ComponentProps<typeof ModuleSaveDialog>> = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  queryClient: QueryClient;
  rerender: (open: boolean) => void;
} {
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const base = {
    mode: "create" as const,
    workspaceSlug: "acme",
    currentProject: CURRENT_PROJECT,
  };
  const ui = (open: boolean): React.ReactElement => (
    <QueryClientProvider client={queryClient}>
      <ModuleSaveDialog {...base} {...props} open={open} onOpenChange={onOpenChange} />
    </QueryClientProvider>
  );
  const view = render(ui(true));
  return { onOpenChange, queryClient, rerender: (open: boolean) => view.rerender(ui(open)) };
}

function titleField(): HTMLElement {
  return screen.getByRole("textbox", { name: "Title" });
}

function submit(label: "Create Module" | "Update Module"): void {
  fireEvent.click(screen.getByRole("button", { name: label }));
}

describe("ModuleSaveDialog — create", () => {
  it("opens blank with defaults and the pinned heading", async () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "Create module" })).toBeInTheDocument();
    expect(titleField()).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("");
    expect(screen.getByRole("button", { name: "Status: Backlog" })).toHaveTextContent("Backlog");
    expect(screen.getByTestId("module-dialog-date-range-trigger")).toHaveTextContent("Start date");
    expect(screen.getByTestId("module-dialog-date-range-trigger")).toHaveTextContent("End date");
    expect(await screen.findByRole("button", { name: "Lead" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Members" })).toBeInTheDocument();
    // The header action opens the dialog with the title in focus.
    expect(titleField()).toHaveFocus();
  });

  it("offers only projects the viewer may create modules in", async () => {
    renderDialog();
    const picker = screen.getByTestId("module-dialog-project");
    fireEvent.click(within(picker).getByRole("button"));
    const options = await screen.findAllByRole("option");
    const names = options.map((option) => option.textContent?.replace(/\s+/g, " ").trim());
    expect(names).toHaveLength(2);
    expect(names[0]).toContain("Website relaunch");
    expect(names[1]).toContain("Project B");
    expect(names).not.toContain("Guest Only");
    expect(names).not.toContain("Modules Off");
    expect(names).not.toContain("Archived");
  });

  it("keeps the project picker out of the numbered keyboard sequence", async () => {
    renderDialog();
    const pickerButton = within(screen.getByTestId("module-dialog-project")).getByRole("button");
    // The popover stamps 0 (natural order); nothing 1..8 is spent on it.
    expect(pickerButton).toHaveAttribute("tabindex", "0");
    const sequence = [
      titleField(),
      screen.getByRole("textbox", { name: "Description" }),
      screen.getByTestId("module-dialog-date-range-trigger"),
      screen.getByRole("button", { name: /Status:/ }),
      screen.getByRole("button", { name: "Lead" }),
      screen.getByRole("button", { name: "Members" }),
      screen.getByRole("button", { name: "Cancel" }),
      screen.getByRole("button", { name: "Create Module" }),
    ] as const;
    expect(sequence.map((element) => element.getAttribute("tabindex"))).toEqual([
      "1",
      "2",
      "3",
      "4",
      "5",
      "6",
      "7",
      "8",
    ]);
  });

  it("refuses an empty title inline and writes nothing", async () => {
    renderDialog();
    submit("Create Module");
    expect(await screen.findByText("Title is required")).toBeInTheDocument();
    expect(harness.writes).toHaveLength(0);
    expect(titleField()).toHaveFocus();
  });

  it("refuses a 256-character title and clears the error as the title becomes valid", async () => {
    renderDialog();
    const field = titleField();
    fireEvent.change(field, { target: { value: "x".repeat(256) } });
    fireEvent.blur(field);
    expect(await screen.findByText("Title must be 255 characters or fewer")).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "Fits" } });
    await waitFor(() => expect(screen.queryByText("Title must be 255 characters or fewer")).not.toBeInTheDocument());
    expect(harness.writes).toHaveLength(0);
  });

  it("closes on Escape and Cancel without writing", async () => {
    const { onOpenChange } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Unsaved" } });
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onOpenChange).toHaveBeenCalledTimes(2);
    expect(harness.writes).toHaveLength(0);
  });

  it("posts every field to the picked project and closes with a notice", async () => {
    const { onOpenChange, queryClient } = renderDialog();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    fireEvent.change(titleField(), { target: { value: "  Full Module  " } });
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), {
      target: { value: "Line one\nLine two" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Status: Backlog" }));
    fireEvent.click(await screen.findByRole("option", { name: /Planned/ }));
    fireEvent.click(screen.getByRole("button", { name: "Lead" }));
    fireEvent.click(await screen.findByRole("option", { name: "You" }));
    fireEvent.click(screen.getByRole("button", { name: "Members" }));
    fireEvent.click(await screen.findByRole("option", { name: "Pat Member" }));
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    submit("Create Module");
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));

    expect(harness.writes).toHaveLength(1);
    expect(harness.writes[0]?.url).toContain(`/projects/${MODULES_PROJECT_ID}/modules/`);
    expect(harness.writes[0]?.body).toEqual({
      name: "Full Module",
      description: "Line one\nLine two",
      start_date: null,
      target_date: null,
      status: "planned",
      lead_id: ME_ID,
      member_ids: [TEAMMATE_ID],
    });
    expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", expect.stringContaining("Full Module"));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: moduleKeys.list("acme", MODULES_PROJECT_ID) });
    expect(onOpenChange).toHaveBeenCalledTimes(1);
  });

  it("sends the dates picked in the range control", async () => {
    renderDialog();
    fireEvent.change(titleField(), { target: { value: "Dated" } });
    fireEvent.click(screen.getByTestId("module-dialog-date-range-trigger"));
    const calendar = screen.getByTestId("module-dialog-date-range-calendar");
    fireEvent.change(within(calendar).getByLabelText("Month"), { target: { value: "9" } });
    fireEvent.change(within(calendar).getByLabelText("Year"), { target: { value: "2026" } });
    fireEvent.click(within(calendar).getByRole("button", { name: "October 1, 2026" }));
    fireEvent.click(within(calendar).getByRole("button", { name: "October 31, 2026" }));
    submit("Create Module");
    await waitFor(() => expect(harness.writes).toHaveLength(1));
    expect(harness.writes[0]?.body).toMatchObject({ start_date: "2026-10-01", target_date: "2026-10-31" });
  });

  it("posts to the project chosen in the picker", async () => {
    const { onOpenChange } = renderDialog();
    fireEvent.click(within(screen.getByTestId("module-dialog-project")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: /Project B/ }));
    fireEvent.change(titleField(), { target: { value: "Elsewhere" } });
    submit("Create Module");
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(harness.writes[0]?.url).toContain(`/projects/${PROJECT_B_ID}/modules/`);
  });

  it("keeps the dialog open with every field intact when the write fails", async () => {
    harness.createStatus = 409;
    const { onOpenChange } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Failed Module" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), {
      target: { value: "Failed description" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Status: Backlog" }));
    fireEvent.click(await screen.findByRole("option", { name: /Paused/ }));
    submit("Create Module");

    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith("Error!", "Module with this name already exists")
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    // NEWFRONT-328: nothing resets — title, description and status stay.
    expect(titleField()).toHaveValue("Failed Module");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("Failed description");
    expect(screen.getByRole("button", { name: "Status: Paused" })).toBeInTheDocument();
    expect(harness.writes).toHaveLength(1);
  });

  it("names the network failure when the refusal carries no sentence", async () => {
    harness.createStatus = 500;
    // A 500 with a bare `error` field still reads through the message.
    renderDialog();
    fireEvent.change(titleField(), { target: { value: "Boom" } });
    submit("Create Module");
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("Error!", expect.any(String)));
  });

  it("remounts blank on the next open after a cancel", async () => {
    const { onOpenChange, rerender } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Unsaved" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    rerender(false);
    rerender(true);
    expect(titleField()).toHaveValue("");
  });

  it("swallows an outside press while a write flies so the form survives", async () => {
    let release: () => void = () => undefined;
    harness.blockWrite = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { onOpenChange } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Pending" } });
    submit("Create Module");
    await screen.findByRole("button", { name: "Creating module…" });
    await userEvent.click(document.querySelector("[data-base-ui-backdrop]") ?? document.body);
    // Only the click-away is held back; Escape still closes.
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(titleField()).toHaveValue("Pending");
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape", code: "Escape" });
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    release();
  });

  it("keeps the dialog up while a write flies, then closes on success", async () => {
    let release: () => void = () => undefined;
    harness.blockWrite = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { onOpenChange } = renderDialog();
    fireEvent.change(titleField(), { target: { value: "Pending" } });
    submit("Create Module");
    await waitFor(() => expect(screen.getByRole("button", { name: "Creating module…" })).toBeInTheDocument());
    // A submitting form cannot be re-submitted, and nothing has closed yet.
    expect(screen.getByRole("button", { name: "Creating module…" })).toBeDisabled();
    expect(onOpenChange).not.toHaveBeenCalled();
    release();
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", expect.stringContaining("Pending"));
  });

  it("waits for the project read before offering a picker selection", async () => {
    harness.projectsStatus = 500;
    renderDialog();
    const picker = screen.getByTestId("module-dialog-project");
    fireEvent.click(within(picker).getByRole("button"));
    await waitFor(() => {
      const alerts = screen.queryAllByRole("alert").map((node) => node.textContent ?? "");
      expect(alerts.join(" ")).toContain("Couldn't load projects.");
    });
  });
});

describe("ModuleSaveDialog — update", () => {
  const record = makeModuleRow({
    name: "Prefill Me",
    description: "Prefill description",
    start_date: "2026-10-01",
    target_date: "2026-10-31",
    status: "planned",
    lead_id: ME_ID,
    member_ids: [TEAMMATE_ID],
  }) as ModuleRow;

  it("opens prefilled without a project picker", async () => {
    renderDialog({ mode: "update", module: record });
    expect(screen.getByRole("heading", { name: "Update module" })).toBeInTheDocument();
    expect(screen.queryByTestId("module-dialog-project")).not.toBeInTheDocument();
    expect(titleField()).toHaveValue("Prefill Me");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("Prefill description");
    expect(screen.getByRole("button", { name: "Status: Planned" })).toBeInTheDocument();
    expect(screen.getByTestId("module-dialog-date-range-trigger")).toHaveTextContent("Oct 1, 2026");
    // A set lead names its member; the members control names the picked one.
    await waitFor(() =>
      expect(within(screen.getByTestId("module-dialog-lead")).getByRole("button")).toHaveTextContent("Rich Owner")
    );
    expect(within(screen.getByTestId("module-dialog-members")).getByRole("button")).toHaveTextContent("Pat Member");
  });

  it("patches the edited fields and closes with a notice", async () => {
    const { onOpenChange, queryClient } = renderDialog({ mode: "update", module: record });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    fireEvent.change(titleField(), { target: { value: "Prefill Edited" } });
    fireEvent.click(screen.getByRole("button", { name: "Status: Planned" }));
    fireEvent.click(await screen.findByRole("option", { name: /In Progress/ }));
    submit("Update Module");
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(harness.writes[0]?.method).toBe("PATCH");
    expect(harness.writes[0]?.url).toContain(`/modules/${MODULES_MODULE_ID}/`);
    expect(harness.writes[0]?.body).toMatchObject({ name: "Prefill Edited", status: "in-progress" });
    expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", "Module updated successfully.");
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: moduleKeys.detail("acme", MODULES_PROJECT_ID, MODULES_MODULE_ID),
    });
  });

  it("keeps every edit intact when the save fails", async () => {
    harness.updateStatus = 400;
    const { onOpenChange } = renderDialog({ mode: "update", module: record });
    fireEvent.change(titleField(), { target: { value: "Save Fail Edited" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Description" }), { target: { value: "Kept" } });
    submit("Update Module");
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("Error!", "Refused."));
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(titleField()).toHaveValue("Save Fail Edited");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("Kept");
  });

  it("starts from the record again on a later open", async () => {
    const { onOpenChange, rerender } = renderDialog({ mode: "update", module: record });
    fireEvent.change(titleField(), { target: { value: "Edited then cancelled" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    rerender(false);
    rerender(true);
    expect(titleField()).toHaveValue("Prefill Me");
  });
});
