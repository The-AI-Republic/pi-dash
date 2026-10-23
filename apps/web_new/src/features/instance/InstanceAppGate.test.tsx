// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { InstanceBootstrap, Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { InstanceAppGate, isAdminEntryPath, isSpacesPath } from "./InstanceAppGate.js";
import { instanceAppKeys } from "./queries.js";

function envelope(setupDone: boolean): InstanceBootstrap {
  return {
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
      is_setup_done: setupDone,
      is_signup_screen_visited: false,
      is_verified: false,
      is_test: false,
      is_current_version_deprecated: false,
      workspaces_exist: true,
    },
  };
}

function response(url: string, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status: 200,
    url,
    headers: { get: () => null },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function renderGate(transport: Transport, queryClient = new QueryClient()): void {
  createAppClient("https://api.test", transport);
  render(
    <QueryClientProvider client={queryClient}>
      <InstanceAppGate>
        <p>{globalThis.window.location.pathname === "/god-mode/" ? "admin entry" : "workspace content"}</p>
      </InstanceAppGate>
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  globalThis.window.history.replaceState(null, "", "/");
});

describe("InstanceAppGate", () => {
  it("treats the console subtree as the administrator entry, and nothing else", () => {
    expect(isAdminEntryPath("/god-mode")).toBe(true);
    expect(isAdminEntryPath("/god-mode/")).toBe(true);
    expect(isAdminEntryPath("/god-mode/workspace/create/")).toBe(true);
    expect(isAdminEntryPath("/acme/settings/members/")).toBe(false);
    expect(isAdminEntryPath("/god-modeish")).toBe(false);
  });

  it("recognises only the public boards subtree", () => {
    expect(isSpacesPath("/spaces")).toBe(true);
    expect(isSpacesPath("/spaces/")).toBe(true);
    expect(isSpacesPath("/spaces/issues/anchor-1/")).toBe(true);
    expect(isSpacesPath("/spaceship")).toBe(false);
    expect(isSpacesPath("/acme/spaces/")).toBe(false);
  });

  it("lets the public boards subtree through without a workspace probe", () => {
    globalThis.window.history.replaceState(null, "", "/spaces/issues/anchor-1/");
    let calls = 0;
    renderGate(() => {
      calls += 1;
      return Promise.resolve(response("https://api.test/api/instances/", envelope(true)));
    });
    expect(screen.getByText("workspace content")).toBeInTheDocument();
    expect(screen.queryByTestId("instance-boot-loader")).not.toBeInTheDocument();
    expect(calls).toBe(0);
  });

  it("shows a full-screen loader until the read settles", async () => {
    let release: (() => void) | undefined;
    renderGate(
      () =>
        new Promise((resolve) => {
          release = () => resolve(response("https://api.test/api/instances/", envelope(true)));
        })
    );
    expect(screen.getByTestId("instance-boot-loader")).toBeInTheDocument();
    expect(screen.queryByText("workspace content")).not.toBeInTheDocument();
    release?.();
    await waitFor(() => expect(screen.getByText("workspace content")).toBeInTheDocument());
  });

  it("shows the boot loader while the not-ready read settles, then the screen", async () => {
    let release: (() => void) | undefined;
    renderGate(
      () =>
        new Promise((resolve) => {
          release = () => resolve(response("https://api.test/api/instances/", envelope(false)));
        })
    );
    expect(screen.getByTestId("instance-boot-loader")).toBeInTheDocument();
    release?.();
    await waitFor(() => expect(screen.getByTestId("instance-not-ready")).toBeInTheDocument());
  });

  it("replaces every route with maintenance when the read fails", async () => {
    renderGate(() =>
      Promise.resolve({
        status: 500,
        url: "https://api.test/api/instances/",
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "down" })),
      })
    );
    await waitFor(() => expect(screen.getByTestId("instance-maintenance")).toBeInTheDocument());
    expect(screen.queryByText("workspace content")).not.toBeInTheDocument();
  });

  it("recovers on its own when a retry after a failed read succeeds", async () => {
    let calls = 0;
    renderGate(() => {
      calls += 1;
      if (calls === 1) {
        return Promise.resolve({
          status: 500,
          url: "https://api.test/api/instances/",
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ detail: "starting" })),
        });
      }
      return Promise.resolve(response("https://api.test/api/instances/", envelope(true)));
    });
    await waitFor(() => expect(screen.getByText("workspace content")).toBeInTheDocument(), { timeout: 10_000 });
    expect(calls).toBe(2);
    expect(screen.queryByTestId("instance-maintenance")).not.toBeInTheDocument();
  }, 15_000);

  it("shows the not-ready screen while setup is unfinished", async () => {
    renderGate(() => Promise.resolve(response("https://api.test/api/instances/", envelope(false))));
    await waitFor(() => expect(screen.getByTestId("instance-not-ready")).toBeInTheDocument());
    expect(screen.queryByText("workspace content")).not.toBeInTheDocument();
  });

  it("renders the app once setup is finished", async () => {
    renderGate(() => Promise.resolve(response("https://api.test/api/instances/", envelope(true))));
    await waitFor(() => expect(screen.getByText("workspace content")).toBeInTheDocument());
  });

  it("prefers a cached data-bearing answer over an error branch", () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(instanceAppKeys.bootstrap, { setupDone: true, bootstrap: envelope(true) });
    renderGate(() => Promise.reject(new Error("must not refetch")), queryClient);
    expect(screen.getByText("workspace content")).toBeInTheDocument();
  });

  it("accepts the backend's bare unprovisioned envelope as not-ready", async () => {
    renderGate(() =>
      Promise.resolve(response("https://api.test/api/instances/", { is_activated: false, is_setup_done: false }))
    );
    await waitFor(() => expect(screen.getByTestId("instance-not-ready")).toBeInTheDocument());
    expect(screen.queryByTestId("instance-maintenance")).not.toBeInTheDocument();
  });

  it("accepts the oracle driver's partial not-ready object", async () => {
    renderGate(() =>
      Promise.resolve(
        response("https://api.test/api/instances/", {
          config: {},
          instance: { is_activated: false, is_setup_done: false },
        })
      )
    );
    await waitFor(() => expect(screen.getByTestId("instance-not-ready")).toBeInTheDocument());
  });

  it("lets the administrator entry through while setup is unfinished", async () => {
    globalThis.window.history.replaceState(null, "", "/god-mode/");
    renderGate(() =>
      Promise.resolve(response("https://api.test/api/instances/", { is_activated: false, is_setup_done: false }))
    );
    expect(await screen.findByText("admin entry")).toBeInTheDocument();
    expect(screen.queryByTestId("instance-not-ready")).not.toBeInTheDocument();
  });

  it("never issues the startup read behind the administrator entry", async () => {
    // The admin gate owns the single instance read on /god-mode
    // (ADM-001/ADM-003): the startup reader's background retry would
    // otherwise re-read behind that screen, so it is never asked for.
    globalThis.window.history.replaceState(null, "", "/god-mode/");
    const seen: string[] = [];
    renderGate((url) => {
      seen.push(new URL(url, "https://api.test").pathname);
      return Promise.resolve({
        status: 500,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "down" })),
      });
    });
    expect(await screen.findByText("admin entry")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(seen).toEqual([]);
  });

  it("lets the administrator entry through on a failed read", async () => {
    globalThis.window.history.replaceState(null, "", "/god-mode/");
    renderGate(() =>
      Promise.resolve({
        status: 500,
        url: "https://api.test/api/instances/",
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "down" })),
      })
    );
    expect(await screen.findByText("admin entry")).toBeInTheDocument();
    expect(screen.queryByTestId("instance-maintenance")).not.toBeInTheDocument();
  });
});
