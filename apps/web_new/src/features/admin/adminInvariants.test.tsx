// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-page invariant audits (ADM-069–071, from
// parity/specs/admin.md §9): the auditors on synthetic fixtures,
// every existing admin screen audited to zeros, and the shared-cache
// single-flight proof. Page slices reuse the auditors in their own
// suites; the area gate proves the same rows in shell.spec.ts.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { AdminSession, Transport } from "@pidash/api-client";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { auditAdminAbsence, auditAdminToggles } from "./adminInvariants.js";
import { useInstanceBootstrap } from "./hooks.js";
import { adminKeys } from "./queries.js";
import { resetAdminStores } from "./stores.js";
import { AdminBrandLoader } from "./components/AdminBrandLoader.js";
import { AdminDashboardLayout } from "./components/AdminDashboardLayout.js";
import { AdminNotFoundScreen } from "./components/AdminNotFoundScreen.js";
import { AdminSetupForm } from "./components/AdminSetupForm.js";
import { AdminSignInForm } from "./components/AdminSignInForm.js";
import { AdminSignOutForm } from "./components/AdminSignOutForm.js";
import { CloudAccessDeniedScreen } from "./components/CloudAccessDeniedScreen.js";
import { CloudEntryErrorScreen } from "./components/CloudEntryErrorScreen.js";
import { InstanceFailureScreen } from "./components/InstanceFailureScreen.js";

function fixture(html: string): ParentNode {
  const host = document.createElement("div");
  host.innerHTML = html;
  return host;
}

describe("auditAdminAbsence", () => {
  it("reports zeros for ordinary controls", () => {
    const root = fixture(
      '<button type="button">Save changes</button>' +
        '<a href="/docs">Read the docs</a>' +
        '<input type="text" value="">' +
        '<button type="button" aria-label="">Retry</button>' +
        '<button type="button"></button>'
    );
    expect(auditAdminAbsence(root)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("counts drag sources, file choosers, and transfer controls", () => {
    const root = fixture(
      '<div draggable="true">row</div>' +
        '<div draggable="false">pinned</div>' +
        '<input type="file">' +
        '<button type="button">Export CSV</button>' +
        '<a href="/import" aria-label="Import workspace">go</a>'
    );
    expect(auditAdminAbsence(root)).toEqual({ draggable: 1, fileInputs: 1, transferControls: 2 });
  });

  it("ignores near-miss names without word boundaries", () => {
    const root = fixture('<button type="button">Important notice</button>' + '<a href="/report">Exporter report</a>');
    expect(auditAdminAbsence(root)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});

describe("auditAdminToggles", () => {
  it("reports no toggles for ordinary buttons", () => {
    const root = fixture('<button type="button">Sign in</button><button type="button">Showcase all</button>');
    expect(auditAdminToggles(root)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("counts toggles and flags the tabbable ones", () => {
    const root = fixture(
      '<button type="button" aria-label="Show password">eye</button>' +
        '<button type="button" aria-label="Hide API key" tabindex="-1">eye</button>'
    );
    expect(auditAdminToggles(root)).toEqual({ toggles: 2, tabbableToggles: 1 });
  });
});

const ADMIN: AdminSession = {
  kind: "admin",
  admin: { email: "admin@example.com", display_name: "Ada Admin", avatar_url: null } as never,
};

function hangingTransport(): Transport {
  return () => new Promise(() => {});
}

function failingTransport(): Transport {
  return (url) =>
    Promise.resolve({
      status: 500,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
    });
}

/** Query client with the native-POST token seeded: no fetch fires. */
function csrfSeededClient(): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-1" });
  return queryClient;
}

function renderLayout(workspacesExist: boolean): void {
  createAppClient("https://api.test", failingTransport());
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(adminKeys.session, ADMIN);
  queryClient.setQueryData(adminKeys.csrf, { csrf_token: "csrf-9" });
  queryClient.setQueryData(adminKeys.bootstrap, {
    instance: { current_version: "9.9.9", workspaces_exist: workspacesExist },
  });
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/god-mode/general",
    component: () => (
      <AdminDashboardLayout>
        <div data-testid="page">general settings</div>
      </AdminDashboardLayout>
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([dashboardRoute]),
    history: createMemoryHistory({ initialEntries: ["/god-mode/general/"] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

function renderNotFound(): void {
  const rootRoute = createRootRoute({ component: AdminNotFoundScreen });
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ["/god-mode/no-such-place/"] }),
    trailingSlash: "preserve",
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  resetAppClient();
});

describe("dashboard shell invariants", () => {
  it("audits the shell to zeros with the popup shut (ADM-070, ADM-071)", async () => {
    renderLayout(true);
    await waitFor(() => expect(screen.getByTestId("admin-dashboard")).toBeInTheDocument());
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("audits the shell to zeros with the first-workspace popup open (ADM-070)", async () => {
    renderLayout(false);
    await waitFor(() => expect(screen.getByTestId("admin-first-workspace-popup")).toBeInTheDocument());
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});

describe("entry form invariants", () => {
  it("keeps both setup toggles out of tab order with no transfer surface (ADM-070, ADM-071)", () => {
    createAppClient("https://api.test", hangingTransport());
    render(
      <QueryClientProvider client={csrfSeededClient()}>
        <AdminSetupForm search={{}} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("admin-setup-form")).toBeInTheDocument();
    expect(auditAdminToggles(document)).toEqual({ toggles: 2, tabbableToggles: 0 });
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("keeps the sign-in toggle out of tab order with no transfer surface (ADM-070, ADM-071)", () => {
    createAppClient("https://api.test", hangingTransport());
    render(
      <QueryClientProvider client={csrfSeededClient()}>
        <AdminSignInForm search={{}} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("admin-sign-in-form")).toBeInTheDocument();
    expect(auditAdminToggles(document)).toEqual({ toggles: 1, tabbableToggles: 0 });
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});

describe("leaf screen invariants", () => {
  it("audits the sign-out control to zeros (ADM-070, ADM-071)", () => {
    createAppClient("https://api.test", hangingTransport());
    render(
      <QueryClientProvider client={csrfSeededClient()}>
        <AdminSignOutForm />
      </QueryClientProvider>
    );
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("audits the cloud denied screen to zeros (ADM-070, ADM-071)", () => {
    render(<CloudAccessDeniedScreen />);
    expect(screen.getByTestId("admin-cloud-denied")).toBeInTheDocument();
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("audits the cloud error screen to zeros (ADM-070, ADM-071)", () => {
    render(<CloudEntryErrorScreen onRetry={() => undefined} />);
    expect(screen.getByTestId("admin-cloud-error")).toBeInTheDocument();
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("audits the fetch-failure screen to zeros (ADM-070, ADM-071)", () => {
    render(<InstanceFailureScreen />);
    expect(screen.getByTestId("admin-failure")).toBeInTheDocument();
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("audits the brand loader to zeros (ADM-070, ADM-071)", () => {
    render(<AdminBrandLoader />);
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });

  it("audits the not-found screen to zeros (ADM-070, ADM-071)", async () => {
    renderNotFound();
    await waitFor(() => expect(screen.getByTestId("admin-not-found")).toBeInTheDocument());
    expect(auditAdminAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    expect(auditAdminToggles(document)).toEqual({ toggles: 0, tabbableToggles: 0 });
  });
});

function BootstrapProbe({ id }: { id: string }): React.ReactElement {
  const query = useInstanceBootstrap();
  if (query.isPending) return <div data-testid={id}>loading</div>;
  if (query.isError) return <div data-testid={id}>failed</div>;
  return <div data-testid={id}>{query.data.instance.is_setup_done ? "set-up" : "first-run"}</div>;
}

const SET_UP_ENVELOPE = {
  config: {
    enable_signup: true,
    is_workspace_creation_disabled: false,
    is_google_enabled: false,
    is_github_enabled: false,
    is_gitlab_enabled: false,
    is_gitea_enabled: false,
    is_magic_login_enabled: false,
    is_email_password_enabled: true,
    is_smtp_configured: false,
    is_self_managed: true,
    instance_changelog_url: "",
  },
  instance: {
    id: "11111111-1111-4111-8111-111111111111",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    instance_name: "Pi Dash",
    whitelist_emails: null,
    instance_id: "instance-1",
    current_version: "1.0.0",
    latest_version: null,
    edition: "oss",
    domain: "example.test",
    last_checked_at: "2026-01-01T00:00:00Z",
    namespace: null,
    is_telemetry_enabled: true,
    is_support_required: false,
    is_setup_done: true,
    is_signup_screen_visited: false,
    is_verified: false,
    is_test: false,
    is_current_version_deprecated: false,
    workspaces_exist: true,
  },
};

describe("shared cached fetch (ADM-069)", () => {
  it("serves every mounted reader from one fetch with no refetch on remount", async () => {
    let fetches = 0;
    createAppClient("https://api.test", ((url: string) => {
      fetches += 1;
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(SET_UP_ENVELOPE)),
      });
    }) as Transport);
    // A bare client on purpose: the pin proves the query options, not
    // provider-level defaults.
    const queryClient = new QueryClient();
    const both = (
      <QueryClientProvider client={queryClient}>
        <BootstrapProbe key="a" id="probe-a" />
        <BootstrapProbe key="b" id="probe-b" />
      </QueryClientProvider>
    );
    const { rerender } = render(both);
    await waitFor(() => expect(screen.getByTestId("probe-a")).toHaveTextContent("set-up"));
    await waitFor(() => expect(screen.getByTestId("probe-b")).toHaveTextContent("set-up"));
    expect(fetches).toBe(1);
    rerender(
      <QueryClientProvider client={queryClient}>
        <BootstrapProbe key="b" id="probe-b" />
      </QueryClientProvider>
    );
    rerender(both);
    await waitFor(() => expect(screen.getByTestId("probe-a")).toHaveTextContent("set-up"));
    expect(fetches).toBe(1);
  });
});
