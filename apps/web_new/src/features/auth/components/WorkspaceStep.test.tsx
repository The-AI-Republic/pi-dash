// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit, WorkspaceInvitationList, WorkspaceJoinRequestList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { WorkspaceStep } from "./WorkspaceStep.js";

interface StubRoute {
  status: number;
  body: string;
  url: string;
}

function stubTransport(handler: (url: string, init: TransportInit) => StubRoute | Promise<StubRoute>): Transport {
  return async (url, init) => {
    const response = await handler(url, init);
    return {
      status: response.status,
      url: response.url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  };
}

const CSRF = JSON.stringify({ csrf_token: "csrf" });

function invitation() {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    workspace: {
      id: "333e4567-e89b-12d3-a456-426614174002",
      name: "Acme",
      slug: "acme",
      logo_url: null,
    },
    email: "ada@example.com",
    accepted: false,
    token: "token",
    message: null,
    responded_at: null,
    role: 15,
    invite_link: "https://app.test/join",
    created_by: null,
    updated_by: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-01-02T03:04:05Z",
  };
}

function joinRequest(adminEmail: string) {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    requester: {
      id: "123e4567-e89b-12d3-a456-426614174000",
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "Ada Lovelace",
      is_bot: false,
    },
    admin_email: adminEmail,
    message: null,
    status: "PENDING",
    responded_at: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-01-02T03:04:05Z",
  };
}

const PROFILE_BODY = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  user: "123e4567-e89b-12d3-a456-426614174000",
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: false,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "full",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: false,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-02-03T04:05:06Z",
};

const CREATED_BODY = {
  id: "333e4567-e89b-12d3-a456-426614174002",
  name: "Acme",
  slug: "acme",
  logo: null,
  logo_asset: null,
  logo_url: null,
  owner: "123e4567-e89b-12d3-a456-426614174000",
  organization_size: "2-10",
  timezone: "UTC",
  background_color: "",
  total_members: 1,
  role: 20,
  created_by: null,
  updated_by: null,
  created_at: "2024-01-02T03:04:05Z",
  updated_at: "2024-01-02T03:04:05Z",
};

const LIST_BODY = [
  {
    id: "333e4567-e89b-12d3-a456-426614174002",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 1,
    role: 20,
  },
];

const SETTINGS_BODY = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  email: "ada@example.com",
  workspace: {
    last_workspace_id: "333e4567-e89b-12d3-a456-426614174002",
    last_workspace_slug: "acme",
    fallback_workspace_id: null,
    fallback_workspace_slug: null,
    invites: 0,
  },
};

interface StepHarness {
  advances: ({ skipInvites?: boolean } | undefined)[];
  rerender: (invitations: WorkspaceInvitationList, joinRequests: WorkspaceJoinRequestList) => void;
}

function renderStep(options: {
  invitations?: WorkspaceInvitationList;
  joinRequests?: WorkspaceJoinRequestList;
  joinAnswer?: unknown;
  creationDisabled?: boolean | "omit";
}): StepHarness {
  const advances: ({ skipInvites?: boolean } | undefined)[] = [];
  const pushAdvance = (advanceOptions: { skipInvites?: boolean } | undefined): void => {
    advances.push(advanceOptions);
  };
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.includes("/api/workspace-slug-check/")) {
        return { status: 200, body: JSON.stringify({ status: true }), url };
      }
      if (url.endsWith("/api/workspaces/") && init.method === "POST") {
        return { status: 201, body: JSON.stringify(CREATED_BODY), url };
      }
      if (url.endsWith("/api/users/me/workspaces/invitations/") && init.method === "POST") {
        return { status: 204, body: "", url };
      }
      if (url.endsWith("/api/users/me/workspaces/join-requests/") && init.method === "POST") {
        return { status: 201, body: JSON.stringify(options.joinAnswer ?? { message: "Request sent" }), url };
      }
      if (url.endsWith("/api/users/me/profile/") && init.method === "PATCH") {
        return { status: 200, body: JSON.stringify(PROFILE_BODY), url };
      }
      if (url.endsWith("/api/users/me/workspaces/")) {
        return { status: 200, body: JSON.stringify(LIST_BODY), url };
      }
      if (url.endsWith("/api/users/me/settings/")) {
        return { status: 200, body: JSON.stringify(SETTINGS_BODY), url };
      }
      return { status: 200, body: "{}", url };
    })
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const creationDisabled = options.creationDisabled === "omit" ? undefined : (options.creationDisabled ?? false);
  const disabledProp = creationDisabled === undefined ? {} : { creationDisabled };
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceStep
        onAdvance={pushAdvance}
        invitations={options.invitations ?? []}
        joinRequests={options.joinRequests ?? []}
        {...disabledProp}
      />
    </QueryClientProvider>
  );
  return {
    advances,
    rerender: (invitations, joinRequests) => {
      view.rerender(
        <QueryClientProvider client={queryClient}>
          <WorkspaceStep
            onAdvance={pushAdvance}
            invitations={invitations}
            joinRequests={joinRequests}
            {...disabledProp}
          />
        </QueryClientProvider>
      );
    },
  };
}

afterEach(() => {
  resetAppClient();
});

describe("WorkspaceStep", () => {
  it("defaults to the create view without invites or requests", () => {
    renderStep({});
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Join by admin email" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Join with an invite" })).not.toBeInTheDocument();
  });

  it("treats an omitted creation flag as enabled", () => {
    renderStep({ creationDisabled: "omit" });
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Workspace creation is turned off" })).not.toBeInTheDocument();
  });

  it("replaces the create view with the restricted notice when creation is disabled", () => {
    renderStep({ creationDisabled: true });
    expect(screen.getByRole("heading", { name: "Workspace creation is turned off" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Create your workspace" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Join by admin email" })).toBeInTheDocument();
  });

  it("moves from the restricted notice into the join-by-email view", async () => {
    const user = userEvent.setup();
    renderStep({ creationDisabled: true });
    await user.click(screen.getByRole("button", { name: "Join by admin email" }));
    expect(screen.getByRole("heading", { name: "Request workspace access" })).toBeInTheDocument();
  });

  it("defaults to the invites view when invitations exist", () => {
    renderStep({ invitations: [invitation()] as unknown as WorkspaceInvitationList });
    expect(screen.getByRole("heading", { name: "Join a workspace" })).toBeInTheDocument();
  });

  it("forces the holding view for a pending request and names the listed address", () => {
    renderStep({ joinRequests: [joinRequest("listed@example.com")] as unknown as WorkspaceJoinRequestList });
    expect(screen.getByRole("heading", { name: "Your request is pending" })).toBeInTheDocument();
    expect(screen.getByText("listed@example.com")).toBeInTheDocument();
  });

  it("resolves pending once the join-request list arrives", () => {
    const harness = renderStep({});
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
    harness.rerender([], [joinRequest("listed@example.com")] as unknown as WorkspaceJoinRequestList);
    expect(screen.getByRole("heading", { name: "Your request is pending" })).toBeInTheDocument();
  });

  it("keeps manual navigation when data arrives late", async () => {
    const harness = renderStep({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Join by admin email" }));
    expect(screen.getByRole("heading", { name: "Request workspace access" })).toBeInTheDocument();
    harness.rerender([invitation()] as unknown as WorkspaceInvitationList, []);
    expect(screen.getByRole("heading", { name: "Request workspace access" })).toBeInTheDocument();
  });

  it("moves between the create and join-by-email views", async () => {
    renderStep({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Join by admin email" }));
    expect(screen.getByRole("heading", { name: "Request workspace access" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back to creating a workspace" }));
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
  });

  it("moves between the create and invites views when invitations exist", async () => {
    renderStep({ invitations: [invitation()] as unknown as WorkspaceInvitationList });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Create a new workspace" }));
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Join with an invite" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Join with an invite" }));
    expect(screen.getByRole("heading", { name: "Join a workspace" })).toBeInTheDocument();
  });

  it("parks on the holding view after a request and returns to creating instead", async () => {
    renderStep({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Join by admin email" }));
    fireEvent.change(screen.getByLabelText("Admin email address"), { target: { value: "admin@example.com" } });
    await user.click(screen.getByRole("button", { name: "Send join request" }));
    expect(await screen.findByRole("heading", { name: "Your request is pending" })).toBeInTheDocument();
    expect(screen.getByText("admin@example.com")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create a workspace instead" }));
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
  });

  it("finishes directly through the already-member shortcut", async () => {
    const harness = renderStep({ joinAnswer: { message: "Already a member", workspace_slug: "acme" } });
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Join by admin email" }));
    fireEvent.change(screen.getByLabelText("Admin email address"), { target: { value: "admin@example.com" } });
    await user.click(screen.getByRole("button", { name: "Send join request" }));
    await waitFor(() => expect(harness.advances).toEqual([{ skipInvites: true }]));
  });

  it("finishes directly after batch-accepting invites", async () => {
    const harness = renderStep({ invitations: [invitation()] as unknown as WorkspaceInvitationList });
    const user = userEvent.setup();
    fireEvent.click(screen.getByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(harness.advances).toEqual([{ skipInvites: true }]));
  });

  it("advances to the invite step after creating with a team size", async () => {
    const harness = renderStep({});
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Acme" } });
    await user.click(screen.getByRole("button", { name: "2-10" }));
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(harness.advances).toEqual([undefined]));
  });

  it("finishes directly after creating with the solo size", async () => {
    const harness = renderStep({});
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Acme" } });
    await user.click(screen.getByRole("button", { name: "Just myself" }));
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(harness.advances).toEqual([{ skipInvites: true }]));
  });
});
