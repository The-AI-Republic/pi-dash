// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { setSessionClient } from "../../../core/session/queries.js";

const { navigate } = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => navigate }));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError } from "../../../shared/shell/index.js";
import { ONBOARDING_STAMP_FAILED_TITLE, OnboardingFlow } from "./OnboardingFlow.js";

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
const UUID = "123e4567-e89b-12d3-a456-426614174000";

const ME = {
  id: UUID,
  email: "ada@example.com",
  username: "ada",
  first_name: "",
  last_name: "",
  display_name: "Ada Byron",
  avatar: "",
  avatar_url: "https://cdn.test/ada.png",
  cover_image: null,
  cover_image_url: null,
  date_joined: "2024-01-02T03:04:05Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "",
  last_login_time: null,
};

function profileFixture(flags: Record<string, boolean> = {}) {
  return {
    id: UUID,
    user: UUID,
    theme: {},
    is_app_rail_docked: false,
    is_tour_completed: false,
    onboarding_step: flags,
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
}

function configFixture(selfManaged: boolean, creationDisabled = false) {
  return {
    enable_signup: true,
    is_workspace_creation_disabled: creationDisabled,
    is_google_enabled: false,
    is_github_enabled: false,
    is_gitlab_enabled: false,
    is_gitea_enabled: false,
    is_magic_login_enabled: false,
    is_email_password_enabled: true,
    is_smtp_configured: true,
    is_self_managed: selfManaged,
    instance_changelog_url: "",
  };
}

const ACME = { id: "w1", name: "Acme", slug: "acme", logo_url: "", total_members: 1, role: 20 };

const SETTINGS = {
  id: UUID,
  email: "ada@example.com",
  workspace: {
    last_workspace_id: "w1",
    last_workspace_slug: "acme",
    fallback_workspace_id: null,
    fallback_workspace_slug: null,
    invites: 0,
  },
};

interface FlowSetup {
  flags?: Record<string, boolean>;
  selfManaged?: boolean;
  creationDisabled?: boolean;
  workspaces?: (typeof ACME)[];
  invitations?: Array<{ id: string }>;
  failProfilePatch?: boolean;
  failOnboard?: boolean;
  profilePatches?: string[];
  steps?: React.ComponentProps<typeof OnboardingFlow>["steps"];
}

function renderFlow(setup: FlowSetup = {}): void {
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.endsWith("/api/users/me/") && init.method === "PATCH") {
        return {
          status: 200,
          body: JSON.stringify({
            id: UUID,
            email: "ada@example.com",
            username: "ada",
            first_name: "Ada",
            last_name: "Byron",
            display_name: "Ada Byron",
            avatar: "",
            user_timezone: "UTC",
          }),
          url,
        };
      }
      if (url.endsWith("/api/users/me/profile/") && init.method === "PATCH") {
        setup.profilePatches?.push(init.body ?? "");
        if (setup.failProfilePatch) return { status: 500, body: "{}", url };
        return { status: 200, body: JSON.stringify(profileFixture()), url };
      }
      if (url.endsWith("/api/users/me/onboard/")) {
        if (setup.failOnboard) return { status: 500, body: "{}", url };
        return { status: 200, body: JSON.stringify({ message: "Updated successfully" }), url };
      }
      if (url.endsWith("/api/users/me/profile/")) {
        return { status: 200, body: JSON.stringify(profileFixture()), url };
      }
      if (url.endsWith("/api/users/me/settings/")) {
        return { status: 200, body: JSON.stringify(SETTINGS), url };
      }
      if (url.endsWith("/auth/set-password/")) {
        return {
          status: 200,
          body: JSON.stringify({ id: UUID, email: "ada@example.com", is_password_autoset: false }),
          url,
        };
      }
      return { status: 200, body: "{}", url };
    })
  );
  setSessionClient({
    getJson: async () => ({}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "me"], ME);
  queryClient.setQueryData(["session", "profile"], profileFixture(setup.flags ?? {}));
  queryClient.setQueryData(["session", "workspaces"], setup.workspaces ?? []);
  queryClient.setQueryData(
    ["instance", "config"],
    configFixture(setup.selfManaged ?? false, setup.creationDisabled ?? false)
  );
  queryClient.setQueryData(["auth", "my-invitations"], setup.invitations ?? []);
  queryClient.setQueryData(["auth", "my-join-requests"], []);
  render(
    <QueryClientProvider client={queryClient}>
      <OnboardingFlow {...(setup.steps === undefined ? {} : { steps: setup.steps })} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  navigate.mockClear();
  vi.mocked(notifyError).mockClear();
});

async function submitProfileName(user: ReturnType<typeof userEvent.setup>, name: string): Promise<void> {
  await user.type(await screen.findByLabelText("Your name"), name);
  await user.click(screen.getByRole("button", { name: "Continue" }));
}

describe("OnboardingFlow", () => {
  it("shows a loader while the account loads", () => {
    createAppClient(
      "https://api.test",
      stubTransport(async () => ({ status: 200, body: "{}", url: "" }))
    );
    setSessionClient({
      getJson: () => new Promise(() => undefined),
      parse: (_schema: unknown, data: unknown) => data,
    } as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <OnboardingFlow />
      </QueryClientProvider>
    );
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.getByText("Loading onboarding")).toBeInTheDocument();
  });

  it("opens on the CLI step with no back and full progress", async () => {
    renderFlow({});
    expect(await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" })).toBeInTheDocument();
    expect(screen.getByText("1/6")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
    expect(screen.getByRole("button", { name: "Account: Ada Byron" })).toBeInTheDocument();
  });

  it("advances from the CLI step to profile and back without storing", async () => {
    const user = userEvent.setup();
    const profilePatches: string[] = [];
    renderFlow({ profilePatches });
    await user.click(await screen.findByRole("button", { name: "Continue" }));

    expect(await screen.findByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
    expect(screen.getByText("2/6")).toBeInTheDocument();
    expect(profilePatches).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" })).toBeInTheDocument();
    expect(screen.getByText("1/6")).toBeInTheDocument();
    expect(profilePatches).toHaveLength(0);
  });

  it("skips the CLI step to profile too", async () => {
    const user = userEvent.setup();
    renderFlow({});
    await user.click(await screen.findByRole("button", { name: "Skip for now" }));
    expect(await screen.findByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
  });

  it("leaves profile setup for role setup off self-managed", async () => {
    const user = userEvent.setup();
    const profilePatches: string[] = [];
    renderFlow({ profilePatches });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");

    expect(await screen.findByRole("heading", { name: "What's your role?" })).toBeInTheDocument();
    expect(screen.getByText("3/6")).toBeInTheDocument();
    // Consent only: the profile-complete stamp waits for the use-case step.
    expect(profilePatches).toHaveLength(1);
    expect(JSON.parse(profilePatches[0] ?? "{}")).toEqual({ has_marketing_email_consent: true });
  });

  it("walks role and use-case forward, saving each", async () => {
    const user = userEvent.setup();
    const profilePatches: string[] = [];
    renderFlow({ profilePatches });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");
    await screen.findByRole("heading", { name: "What's your role?" });

    await user.click(screen.getByRole("radio", { name: "Developer" }));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { name: "What brings you to Pi Dash?" })).toBeInTheDocument();
    expect(screen.getByText("4/6")).toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: "Manage engineering sprints" }));
    await user.click(screen.getByRole("checkbox", { name: "Just exploring" }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // The workspace step mounts in S8; the shell holds its place meanwhile.
    await waitFor(() => expect(screen.getByText("5/6")).toBeInTheDocument());
    expect(screen.queryByRole("heading")).toBeNull();
    expect(profilePatches.map((body) => JSON.parse(body))).toContainEqual({ role: "developer" });
    expect(profilePatches.map((body) => JSON.parse(body))).toContainEqual({
      use_case: "Manage engineering sprints. Just exploring",
    });
    expect(profilePatches.map((body) => JSON.parse(body))).toContainEqual({
      onboarding_step: { profile_complete: true },
    });
  });

  it("skips role and use-case without storing their values", async () => {
    const user = userEvent.setup();
    const profilePatches: string[] = [];
    renderFlow({ profilePatches });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");
    await screen.findByRole("heading", { name: "What's your role?" });

    await user.click(screen.getByRole("button", { name: "Skip" }));
    expect(await screen.findByRole("heading", { name: "What brings you to Pi Dash?" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Skip" }));

    await waitFor(() => expect(screen.getByText("5/6")).toBeInTheDocument());
    const parsed = profilePatches.map((body) => JSON.parse(body) as Record<string, unknown>);
    expect(parsed.some((patch) => "role" in patch)).toBe(false);
    expect(parsed.some((patch) => "use_case" in patch)).toBe(false);
    // The transition stamp still lands so a refresh resumes here.
    expect(parsed).toContainEqual({ onboarding_step: { profile_complete: true } });
  });

  it("steps back from the workspace step to use-case off self-managed", async () => {
    const user = userEvent.setup();
    renderFlow({ flags: { profile_complete: true } });
    await waitFor(() => expect(screen.getByText("5/6")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByRole("heading", { name: "What brings you to Pi Dash?" })).toBeInTheDocument();
  });

  it("jumps profile to workspace on self-managed, stepping back to profile", async () => {
    const user = userEvent.setup();
    const profilePatches: string[] = [];
    renderFlow({ selfManaged: true, profilePatches });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");

    await waitFor(() => expect(screen.getByText("3/4")).toBeInTheDocument());
    expect(profilePatches.map((body) => JSON.parse(body))).toContainEqual({
      onboarding_step: { profile_complete: true },
    });
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
  });

  it("finishes at once for self-managed members and lands home", async () => {
    const user = userEvent.setup();
    renderFlow({ selfManaged: true, workspaces: [ACME] });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");

    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/acme" }));
  });

  it("resumes a created workspace at the invite step with no back", async () => {
    renderFlow({ flags: { profile_complete: true, workspace_create: true } });
    await waitFor(() => expect(screen.getByText("6/6")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
  });

  it("toasts but still moves when a progress stamp fails", async () => {
    const user = userEvent.setup();
    renderFlow({ selfManaged: true, failProfilePatch: true });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");

    await waitFor(() => expect(screen.getByText("3/4")).toBeInTheDocument());
    expect(notifyError).toHaveBeenCalledWith(ONBOARDING_STAMP_FAILED_TITLE);
  });

  it("stays on the step when finishing fails", async () => {
    const user = userEvent.setup();
    renderFlow({ selfManaged: true, workspaces: [ACME], failOnboard: true });
    await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await submitProfileName(user, "Ada Byron");

    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(navigate).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
  });

  it("starts at profile setup with no back on desktop", async () => {
    const kind = platform.kind;
    platform.kind = "desktop";
    try {
      renderFlow({});
      expect(await screen.findByRole("heading", { name: "Create your profile" })).toBeInTheDocument();
      expect(screen.getByText("1/5")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
    } finally {
      platform.kind = kind;
    }
  });

  it("renders fallbacks when every read fails", async () => {
    createAppClient(
      "https://api.test",
      stubTransport(async () => ({ status: 200, body: "{}", url: "" }))
    );
    setSessionClient({
      getJson: async () => {
        throw new Error("offline");
      },
      parse: (_schema: unknown, data: unknown) => data,
    } as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <OnboardingFlow />
      </QueryClientProvider>
    );
    // Fresh-user defaults: the CLI step first, no invitations, no workspaces.
    expect(await screen.findByRole("heading", { name: "Connect your AI agents with Pi Dash" })).toBeInTheDocument();
    expect(screen.getByText("1/6")).toBeInTheDocument();
  });

  it("mounts later steps through the registry", async () => {
    const user = userEvent.setup();
    const seen: Array<{ invitations: unknown[]; joinRequests: unknown[] }> = [];
    renderFlow({
      flags: { profile_complete: true },
      steps: {
        workspace: (props) => {
          seen.push({ invitations: props.invitations, joinRequests: props.joinRequests });
          return (
            <button type="button" onClick={() => props.onAdvance()}>
              to-invite
            </button>
          );
        },
      },
    });
    await screen.findByRole("button", { name: "to-invite" });
    expect(seen).toEqual([{ invitations: [], joinRequests: [] }]);
    await user.click(screen.getByRole("button", { name: "to-invite" }));
    await waitFor(() => expect(screen.getByText("6/6")).toBeInTheDocument());
  });

  it("passes the creation-disabled flag to mounted steps", async () => {
    const seen: unknown[] = [];
    renderFlow({
      flags: { profile_complete: true },
      creationDisabled: true,
      steps: {
        workspace: (props) => {
          seen.push(props.creationDisabled);
          return <div data-testid="workspace-step" />;
        },
      },
    });
    await screen.findByTestId("workspace-step");
    expect(seen).toEqual([true]);
  });

  it("hides the invite step from the total while invitations exist", async () => {
    renderFlow({ flags: { profile_complete: true }, invitations: [{ id: "invite-1" }] });
    await waitFor(() => expect(screen.getByText("5/5")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Back" })).toBeInTheDocument();
  });

  it("finishes from a mounted step through the skip-invites shortcut", async () => {
    const user = userEvent.setup();
    renderFlow({
      flags: { profile_complete: true },
      steps: {
        workspace: (props) => (
          <button type="button" onClick={() => props.onAdvance({ skipInvites: true })}>
            finish-now
          </button>
        ),
      },
    });
    await user.click(await screen.findByRole("button", { name: "finish-now" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/create-workspace" }));
  });
});
