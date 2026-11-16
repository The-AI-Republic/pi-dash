// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import { ProjectAccessGate } from "./ProjectAccessGate.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function membershipRow(role: number) {
  return {
    id: UUID,
    workspace: UUID,
    member: UUID,
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
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    archived_at: null,
    workspace: UUID,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
  };
}

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

interface GateHarness {
  detail: (url: string) => StubRoute | Promise<StubRoute>;
  membershipRole: number | null;
  joinStatus?: number | undefined;
  joinGate?: Promise<void> | undefined;
  onJoin?: (() => void) | undefined;
}

function renderGate(harness: GateHarness): { joinPosts: Array<{ url: string; body: string }> } {
  const joinPosts: Array<{ url: string; body: string }> = [];
  const transport: Transport = async (url, init) => {
    let response: StubRoute;
    if (url.endsWith("/auth/get-csrf-token/")) {
      response = { status: 200, url, body: JSON.stringify({ csrf_token: "csrf" }) };
    } else if (url === `https://api.test/api/workspaces/acme/projects/${PROJECT_ID}/`) {
      response = await harness.detail(url);
    } else if (url.endsWith("/workspace-members/me/")) {
      const row = harness.membershipRole === null ? null : membershipRow(harness.membershipRole);
      response = { status: 200, url, body: JSON.stringify(row) };
    } else if (url.endsWith("/projects/invitations/")) {
      await harness.joinGate;
      harness.onJoin?.();
      joinPosts.push({ url, body: String(init.body ?? "") });
      response = { status: harness.joinStatus ?? 201, url, body: JSON.stringify({ message: "joined" }) };
    } else {
      response = { status: 404, url, body: "{}" };
    }
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectAccessGate slug="acme" projectId={PROJECT_ID}>
        <p>Project content</p>
      </ProjectAccessGate>
    </QueryClientProvider>
  );
  return { joinPosts };
}

function denied(status: number, error: string): (url: string) => StubRoute {
  return (url: string) => ({ status, url, body: JSON.stringify({ error }) });
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
});

describe("ProjectAccessGate", () => {
  it("renders nothing while the details load", () => {
    createAppClient("https://api.test", async (url) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () => JSON.stringify({ csrf_token: "csrf" }),
        };
      }
      await new Promise<void>(() => undefined);
      return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <ProjectAccessGate slug="acme" projectId={PROJECT_ID}>
          <p>Project content</p>
        </ProjectAccessGate>
      </QueryClientProvider>
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders children for members", async () => {
    renderGate({
      detail: (url) => ({ status: 200, url, body: JSON.stringify(projectRow()) }),
      membershipRole: 15,
    });
    await screen.findByText("Project content");
  });

  it("joins on confirm and renders the project once the join lands", async () => {
    let joined = false;
    const seen = renderGate({
      detail: (url) =>
        joined
          ? { status: 200, url, body: JSON.stringify(projectRow()) }
          : denied(409, "You are not a member of this project")(url),
      membershipRole: 15,
      onJoin: () => {
        joined = true;
      },
    });

    const join = await screen.findByRole("button", { name: "Join project" });
    expect(join).toHaveFocus();
    await userEvent.setup().click(join);
    await screen.findByText("Project content");
    expect(seen.joinPosts).toHaveLength(1);
    expect(seen.joinPosts[0]?.url).toBe("https://api.test/api/users/me/workspaces/acme/projects/invitations/");
    expect(seen.joinPosts[0]?.body).toContain(PROJECT_ID);
  });

  it("disables joining while the join is in flight", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    renderGate({
      detail: denied(409, "You are not a member of this project"),
      membershipRole: 15,
      joinGate: gate,
    });
    await userEvent.setup().click(await screen.findByRole("button", { name: "Join project" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Joining…" })).toBeDisabled());
    release();
    await waitFor(() => expect(screen.getByRole("button", { name: "Join project" })).toBeEnabled());
  });

  it("offers joining to admins facing a private project", async () => {
    renderGate({ detail: denied(403, "You do not have permission"), membershipRole: 20 });
    await screen.findByRole("button", { name: "Join project" });
  });

  it("explains without a join action to refused non-admins", async () => {
    renderGate({ detail: denied(403, "You do not have permission"), membershipRole: 15 });
    await screen.findByRole("heading", { name: "You can't open this project" });
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("treats a missing membership as non-admin", async () => {
    renderGate({ detail: denied(403, "You do not have permission"), membershipRole: null });
    await screen.findByRole("heading", { name: "You can't open this project" });
  });

  it("reports not-found for unknown projects", async () => {
    renderGate({ detail: denied(404, "Project does not exist"), membershipRole: 15 });
    await screen.findByRole("heading", { name: "Project not found" });
  });

  it("reports not-found for anything else, including transport failures", async () => {
    renderGate({
      detail: () => {
        throw new Error("boom");
      },
      membershipRole: 15,
    });
    await screen.findByRole("heading", { name: "Project not found" });
  });

  it("stays on the join screen when the join fails", async () => {
    renderGate({
      detail: denied(409, "You are not a member of this project"),
      membershipRole: 15,
      joinStatus: 500,
    });
    await userEvent.setup().click(await screen.findByRole("button", { name: "Join project" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Join project" })).toBeEnabled();
  });
});
