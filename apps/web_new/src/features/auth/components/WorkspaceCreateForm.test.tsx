// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CreatedWorkspace, Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { SLUG_FORMAT_MESSAGE, SLUG_TAKEN_MESSAGE, WORKSPACE_TEAM_SIZES } from "../workspace.js";
import {
  CREATE_FAILED_TITLE,
  CREATE_SUCCESS_TITLE,
  TEAM_SIZE_REQUIRED_MESSAGE,
  WorkspaceCreateForm,
  type TeamSizeSelectorField,
} from "./WorkspaceCreateForm.js";

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
  updated_at: "2024-02-03T04:05:06Z",
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

interface FormHarness {
  created: { workspace: CreatedWorkspace; size: string }[];
  seen: string[];
  posted: Record<string, unknown>;
  onCreated: (workspace: CreatedWorkspace, size: string) => void;
}

function renderForm(options: {
  slugStatus?: boolean;
  failSlugCheck?: boolean;
  failCreate?: boolean;
  gateSlugCheck?: { gate: Promise<StubRoute> };
  renderSizeSelector?: (field: TeamSizeSelectorField) => React.ReactElement;
  captureField?: (field: TeamSizeSelectorField) => void;
}): FormHarness {
  const harness: FormHarness = {
    created: [],
    seen: [],
    posted: {},
    onCreated: (workspace, size) => harness.created.push({ workspace, size }),
  };
  createAppClient(
    "https://api.test",
    stubTransport(async (url, init) => {
      harness.seen.push(`${init.method} ${url}`);
      if (url.endsWith("/auth/get-csrf-token/")) return { status: 200, body: CSRF, url };
      if (url.includes("/api/workspace-slug-check/")) {
        if (options.gateSlugCheck) return options.gateSlugCheck.gate;
        if (options.failSlugCheck) return { status: 500, body: "{}", url };
        return { status: 200, body: JSON.stringify({ status: options.slugStatus ?? true }), url };
      }
      if (url.endsWith("/api/workspaces/") && init.method === "POST") {
        harness.posted["create"] = JSON.parse(init.body as string);
        if (options.failCreate) return { status: 400, body: JSON.stringify({ slug: ["taken"] }), url };
        return { status: 201, body: JSON.stringify(CREATED_BODY), url };
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
  const renderSizeSelector = options.renderSizeSelector
    ? (field: TeamSizeSelectorField) => {
        options.captureField?.(field);
        return options.renderSizeSelector!(field);
      }
    : undefined;
  render(
    <QueryClientProvider client={queryClient}>
      <WorkspaceCreateForm onCreated={harness.onCreated} {...(renderSizeSelector ? { renderSizeSelector } : {})} />
    </QueryClientProvider>
  );
  return harness;
}

function fillValid(user: ReturnType<typeof userEvent.setup>) {
  return (async () => {
    await user.clear(screen.getByLabelText("Workspace name"));
    await user.type(screen.getByLabelText("Workspace name"), "Acme");
    await user.click(screen.getByRole("button", { name: "2-10" }));
  })();
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifyError).mockClear();
  vi.mocked(notifySuccess).mockClear();
});

describe("WorkspaceCreateForm", () => {
  it("renders the fields with every team-size pill and no errors", () => {
    renderForm({});
    expect(screen.getByRole("heading", { name: "Create your workspace" })).toBeInTheDocument();
    expect(screen.getByLabelText("Workspace name")).toBeInTheDocument();
    expect(screen.getByLabelText("Workspace URL")).toBeInTheDocument();
    expect(screen.getByRole("radiogroup", { name: "Team size" })).toBeInTheDocument();
    for (const size of WORKSPACE_TEAM_SIZES) {
      expect(screen.getByRole("button", { name: size })).toBeInTheDocument();
    }
    expect(screen.queryByText(SLUG_FORMAT_MESSAGE)).not.toBeInTheDocument();
    expect(screen.queryByText(SLUG_TAKEN_MESSAGE)).not.toBeInTheDocument();
  });

  it("derives the slug from the name but keeps manual edits", async () => {
    renderForm({});
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Workspace name"), "My Cool Team");
    expect(screen.getByLabelText("Workspace URL")).toHaveValue("my-cool-team");

    await user.clear(screen.getByLabelText("Workspace URL"));
    await user.type(screen.getByLabelText("Workspace URL"), "custom-slug");
    expect(screen.getByLabelText("Workspace URL")).toHaveValue("custom-slug");

    await user.type(screen.getByLabelText("Workspace name"), " More");
    expect(screen.getByLabelText("Workspace URL")).toHaveValue("custom-slug");
  });

  it("lowercases typed slugs", () => {
    renderForm({});
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "Mixed-Case" } });
    expect(screen.getByLabelText("Workspace URL")).toHaveValue("mixed-case");
  });

  it("shows the name error live once edited", () => {
    renderForm({});
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Bad.Name" } });
    expect(screen.getByText("Use only letters, numbers, spaces, hyphens, and underscores.")).toBeInTheDocument();
  });

  it("blocks submit inline for a malformed slug", () => {
    renderForm({});
    expect(SLUG_FORMAT_MESSAGE).toContain("URLs can contain only");
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Bad One" } });
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "bad@slug" } });
    expect(screen.getByText(SLUG_FORMAT_MESSAGE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create workspace" })).toBeDisabled();
  });

  it("reveals every gap on an empty submit without calling the server", async () => {
    const harness = renderForm({});
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    expect(screen.getByText("Enter a workspace name.")).toBeInTheDocument();
    expect(screen.getByText("Enter a workspace URL.")).toBeInTheDocument();
    expect(screen.getByText(TEAM_SIZE_REQUIRED_MESSAGE)).toBeInTheDocument();
    expect(harness.seen).toHaveLength(0);
    expect(harness.created).toHaveLength(0);
  });

  it("rejects a reserved slug inline without calling the server", async () => {
    const harness = renderForm({});
    const user = userEvent.setup();
    expect(SLUG_TAKEN_MESSAGE).toContain("already taken");
    await fillValid(user);
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "api" } });
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    expect(await screen.findByText(SLUG_TAKEN_MESSAGE)).toBeInTheDocument();
    expect(harness.seen).toHaveLength(0);
    expect(harness.created).toHaveLength(0);
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("rejects a taken slug inline and creates nothing", async () => {
    const harness = renderForm({ slugStatus: false });
    const user = userEvent.setup();
    await fillValid(user);
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "taken" } });
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    expect(await screen.findByText(SLUG_TAKEN_MESSAGE)).toBeInTheDocument();
    expect(harness.seen.some((entry) => entry.includes("/api/workspace-slug-check/"))).toBe(true);
    expect(harness.seen.some((entry) => entry === "POST https://api.test/api/workspaces/")).toBe(false);
    expect(harness.created).toHaveLength(0);
  });

  it("clears the taken error when the slug is edited", async () => {
    renderForm({ slugStatus: false });
    const user = userEvent.setup();
    await fillValid(user);
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "taken" } });
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    expect(await screen.findByText(SLUG_TAKEN_MESSAGE)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Workspace URL"), { target: { value: "taken-2" } });
    expect(screen.queryByText(SLUG_TAKEN_MESSAGE)).not.toBeInTheDocument();
  });

  it("creates with a trimmed name, toasts, and reports the workspace and size", async () => {
    const harness = renderForm({});
    const user = userEvent.setup();
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "  Acme  " } });
    await user.click(screen.getByRole("button", { name: "2-10" }));
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(harness.created).toHaveLength(1));
    expect(harness.created[0]?.workspace.slug).toBe("acme");
    expect(harness.created[0]?.size).toBe("2-10");
    expect(harness.posted["create"]).toEqual({ name: "Acme", slug: "acme", organization_size: "2-10" });
    expect(notifySuccess).toHaveBeenCalledWith(CREATE_SUCCESS_TITLE);
  });

  it("toasts the server detail when the slug check fails", async () => {
    const harness = renderForm({ failSlugCheck: true });
    const user = userEvent.setup();
    await fillValid(user);
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(CREATE_FAILED_TITLE, expect.any(String));
    expect(harness.created).toHaveLength(0);
  });

  it("toasts the server detail when creation fails", async () => {
    const harness = renderForm({ failCreate: true });
    const user = userEvent.setup();
    await fillValid(user);
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(CREATE_FAILED_TITLE, expect.any(String));
    expect(harness.created).toHaveLength(0);
  });

  it("disables the pills and submit while the availability check is in flight", async () => {
    let release!: (route: StubRoute) => void;
    const gate = new Promise<StubRoute>((resolve) => {
      release = resolve;
    });
    const harness = renderForm({ gateSlugCheck: { gate } });
    const user = userEvent.setup();
    await fillValid(user);
    const pending = user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "2-10" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Create workspace" })).toBeDisabled();
    release({ status: 200, body: JSON.stringify({ status: true }), url: "https://api.test/api/workspace-slug-check/" });
    await pending;
    await waitFor(() => expect(harness.created).toHaveLength(1));
  });

  it("renders a custom size selector through the S9 seam", async () => {
    const seenFields: TeamSizeSelectorField[] = [];
    const harness = renderForm({
      captureField: (field) => seenFields.push(field),
      renderSizeSelector: (field) => (
        <select
          aria-label="Team size"
          value={field.value}
          disabled={field.disabled}
          onChange={(event) => field.onSelect(event.target.value)}
        >
          <option value="">Pick one</option>
          {field.options.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      ),
    });
    const user = userEvent.setup();
    expect(screen.queryByRole("radiogroup", { name: "Team size" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Workspace name"), { target: { value: "Acme" } });
    fireEvent.change(screen.getByLabelText("Team size"), { target: { value: "11-50" } });
    await user.click(screen.getByRole("button", { name: "Create workspace" }));
    await waitFor(() => expect(harness.created).toHaveLength(1));
    expect(harness.created[0]?.size).toBe("11-50");
    expect(seenFields.length).toBeGreaterThan(0);
    expect(seenFields[0]?.options).toEqual([...WORKSPACE_TEAM_SIZES]);
    expect(seenFields[0]?.labelId).toBe("create-workspace-size-label");
  });
});
