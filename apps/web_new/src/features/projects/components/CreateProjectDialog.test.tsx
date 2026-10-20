// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails, WorkspaceMember } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CREATED_TITLE } from "../api/mutations.js";
import { CREATE_ICON_CHOICES } from "../lib/project-create.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { CreateProjectDialog } from "./CreateProjectDialog.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const LEAD_ID = "323e4567-e89b-12d3-a456-426614174002";
const MATE_ID = "423e4567-e89b-12d3-a456-426614174003";

function createdRow(): ProjectDetails {
  return {
    id: PROJECT_ID,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name: "Nebula Craft",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "NEBULA",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
  };
}

function member(id: string, displayName: string): WorkspaceMember {
  return {
    id: `aaaaaaaa-0000-4000-8000-${id.slice(0, 12)}`,
    member: {
      id,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: displayName,
      is_bot: false,
    },
    role: 15,
  };
}

const MEMBERS = [member(LEAD_ID, "Ada Lovelace"), member(MATE_ID, "Grace Hopper")];

function stubClient(responder: (url: string, method: string) => { status: number; body: string }) {
  const calls: string[] = [];
  const bodies: string[] = [];
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    calls.push(`${method} ${url}`);
    if (typeof init?.body === "string") bodies.push(init.body);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    const { status, body } = responder(url, method);
    return { status, url, headers: { get: () => null }, text: async () => body };
  }) as never);
  return { calls, bodies };
}

function succeed() {
  return stubClient(() => ({ status: 201, body: JSON.stringify(createdRow()) }));
}

function failWith(body: string) {
  return stubClient(() => ({ status: 400, body }));
}

function setup(members: WorkspaceMember[] | undefined = MEMBERS) {
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <CreateProjectDialog workspaceSlug="acme" members={members} open onOpenChange={onOpenChange} />
    </QueryClientProvider>
  );
  return { onOpenChange, ...view };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("CreateProjectDialog", () => {
  it("shows a fresh form with a prefilled icon, timezone and public visibility", () => {
    succeed();
    setup();

    expect(screen.getByRole("heading", { name: "New project" })).not.toBeNull();
    const icon = screen.getByRole("button", { name: "Project icon" });
    expect(CREATE_ICON_CHOICES).toContain(icon.textContent);
    expect(screen.getByText(/^Timezone:/).textContent).not.toBe("Timezone: ");
    expect(screen.getByRole("button", { name: "Visibility" })).toHaveTextContent("Public");
    expect(screen.getByRole("button", { name: "Project lead" })).toHaveTextContent("Lead");
    expect((screen.getByPlaceholderText("Project name") as HTMLInputElement).value).toBe("");
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("");
  });

  it("derives the short code from the name until hand-edited", () => {
    succeed();
    setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft" } });
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("NebulaCraf");

    fireEvent.change(screen.getByPlaceholderText("Project ID"), { target: { value: "NEB!" } });
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("NEB");

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft Two" } });
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("NEB");
  });

  it("clears the code when the name is cleared while syncing", () => {
    succeed();
    setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula" } });
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("Nebula");
    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "" } });
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("");
  });

  it("blocks an empty submit with field errors and no request", () => {
    const { calls } = succeed();
    setup();

    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    expect(screen.getByText("Enter a project name.")).not.toBeNull();
    expect(screen.getByText("Enter a short code.")).not.toBeNull();
    expect(calls).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("creates with one POST carrying identity, visibility, lead, icon and timezone", async () => {
    const { calls, bodies } = succeed();
    const { onOpenChange } = setup();

    fireEvent.click(screen.getByRole("button", { name: "Visibility" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Private" }));
    fireEvent.click(screen.getByRole("button", { name: "Project lead" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Ada Lovelace" }));
    fireEvent.click(screen.getByRole("button", { name: "Project icon" }));
    fireEvent.click(screen.getByRole("button", { name: `Use ${CREATE_ICON_CHOICES[3]} as the project icon` }));
    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "  Nebula Craft " } });
    fireEvent.change(screen.getByPlaceholderText("Description"), { target: { value: "Team space" } });

    const timezoneLine = screen.getByText(/^Timezone:/).textContent ?? "";
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(CREATED_TITLE, "Project created successfully"));

    const posts = calls.filter((call) => call.startsWith("POST") && call.endsWith("/projects/"));
    expect(posts).toHaveLength(1);
    expect(bodies).toHaveLength(1);
    const payload = JSON.parse(bodies[0] ?? "{}") as Record<string, unknown>;
    expect(payload).toMatchObject({
      name: "Nebula Craft",
      identifier: "NEBULACRAF",
      description: "Team space",
      network: 0,
      project_lead: LEAD_ID,
      logo_props: { in_use: "emoji", emoji: { value: CREATE_ICON_CHOICES[3] } },
      timezone: timezoneLine.replace("Timezone: ", ""),
      cycle_view: false,
      module_view: false,
      issue_views_view: false,
      page_view: false,
      intake_view: false,
    });
    for (const key of Object.keys(payload)) {
      expect(key.startsWith("cover_")).toBe(false);
    }
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes on success with no feature step (PSET-048)", async () => {
    const { calls } = succeed();
    const { onOpenChange } = setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft" } });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));

    expect(notifySuccess).toHaveBeenCalledWith(CREATED_TITLE, "Project created successfully");
    // The removed post-create sheet was the only home of the combined
    // five-switch list; creation never grows a follow-up step or a
    // second project write.
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("button", { name: "Open project" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Projects and work items" })).toBeNull();
    expect(calls.filter((call) => call.startsWith("POST") || call.startsWith("PATCH"))).toHaveLength(1);
  });

  it("maps a reused name onto the name field and stays open", async () => {
    failWith(JSON.stringify({ name: ["PROJECT_NAME_ALREADY_EXIST"] }));
    const { onOpenChange } = setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft" } });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(screen.getByText("The project name is already taken.")).not.toBeNull());
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("maps a reused code onto the identifier field and stays open", async () => {
    failWith(JSON.stringify({ identifier: ["PROJECT_IDENTIFIER_ALREADY_EXIST"] }));
    const { onOpenChange } = setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft" } });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(screen.getByText("The project identifier is already taken.")).not.toBeNull());
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("toasts other failures without field errors", async () => {
    failWith(JSON.stringify({ detail: "workspace gone" }));
    const { onOpenChange } = setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft" } });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't create the project", "workspace gone"));
    expect(screen.queryByText("The project name is already taken.")).toBeNull();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("clears a taken error once the field is edited", async () => {
    failWith(JSON.stringify({ name: ["PROJECT_NAME_ALREADY_EXIST"] }));
    setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft" } });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(screen.getByText("The project name is already taken.")).not.toBeNull());

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula Craft Two" } });
    expect(screen.queryByText("The project name is already taken.")).toBeNull();
  });

  it("shows the working label while creation flies", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      await gate;
      return { status: 201, url, headers: { get: () => null }, text: async () => JSON.stringify(createdRow()) };
    }) as never);
    setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula" } });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Creating…" })).not.toBeNull());
    release();
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
  });

  it("marks the picked visibility and lead in their menus", () => {
    succeed();
    setup();

    fireEvent.click(screen.getByRole("button", { name: "Visibility" }));
    expect(screen.getByRole("menuitem", { name: "Public" }).querySelector("svg")).not.toBeNull();
    expect(screen.getByRole("menuitem", { name: "Private" }).querySelector("svg")).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "Private" }));
    expect(screen.getByRole("button", { name: "Visibility" })).toHaveTextContent("Private");

    fireEvent.click(screen.getByRole("button", { name: "Visibility" }));
    expect(screen.getByRole("menuitem", { name: "Private" }).querySelector("svg")).not.toBeNull();
    expect(screen.getByRole("menuitem", { name: "Public" }).querySelector("svg")).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "Public" }));

    fireEvent.click(screen.getByRole("button", { name: "Project lead" }));
    expect(screen.getByRole("menuitem", { name: "No lead" }).querySelector("svg")).not.toBeNull();
    expect(screen.getByRole("menuitem", { name: "Ada Lovelace" }).querySelector("svg")).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "Ada Lovelace" }));
    expect(screen.getByRole("button", { name: "Project lead" })).toHaveTextContent("Ada Lovelace");

    fireEvent.click(screen.getByRole("button", { name: "Project lead" }));
    expect(screen.getByRole("menuitem", { name: "Ada Lovelace" }).querySelector("svg")).not.toBeNull();
    expect(screen.getByRole("menuitem", { name: "No lead" }).querySelector("svg")).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: "No lead" }));
    expect(screen.getByRole("button", { name: "Project lead" })).toHaveTextContent("Lead");
  });

  it("marks the picked icon pressed in the picker", () => {
    succeed();
    setup();

    fireEvent.click(screen.getByRole("button", { name: "Project icon" }));
    const current = screen.getByRole("button", { name: "Project icon" }).textContent ?? "";
    const other = CREATE_ICON_CHOICES.find((choice) => choice !== current) ?? CREATE_ICON_CHOICES[0] ?? "";
    const currentButton = screen.getByRole("button", { name: `Use ${current} as the project icon` });
    expect(currentButton.getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: `Use ${other} as the project icon` }));
    expect(screen.getByRole("button", { name: "Project icon" })).toHaveTextContent(other);
  });

  it("disables the lead picker while members resolve", () => {
    succeed();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <CreateProjectDialog workspaceSlug="acme" members={undefined} open onOpenChange={vi.fn()} />
      </QueryClientProvider>
    );

    expect(screen.getByRole("button", { name: "Project lead" }).hasAttribute("disabled")).toBe(true);
  });

  it("falls back to the lead placeholder when the picked member is gone", () => {
    succeed();
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={queryClient}>
        <CreateProjectDialog workspaceSlug="acme" members={MEMBERS} open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );

    fireEvent.click(screen.getByRole("button", { name: "Project lead" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Ada Lovelace" }));
    expect(screen.getByRole("button", { name: "Project lead" })).toHaveTextContent("Ada Lovelace");

    view.rerender(
      <QueryClientProvider client={queryClient}>
        <CreateProjectDialog workspaceSlug="acme" members={[]} open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Project lead" })).toHaveTextContent("Lead");
  });

  it("cancels without writing", () => {
    const { calls } = succeed();
    const { onOpenChange } = setup();

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(calls).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("starts each visit with a fresh form", () => {
    succeed();
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={queryClient}>
        <CreateProjectDialog workspaceSlug="acme" members={MEMBERS} open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );

    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Nebula" } });
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <CreateProjectDialog workspaceSlug="acme" members={MEMBERS} open={false} onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <CreateProjectDialog workspaceSlug="acme" members={MEMBERS} open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );
    expect((screen.getByPlaceholderText("Project name") as HTMLInputElement).value).toBe("");
    expect((screen.getByPlaceholderText("Project ID") as HTMLInputElement).value).toBe("");
    expect(screen.getByRole("button", { name: "Project lead" })).toHaveTextContent("Lead");
  });
});
