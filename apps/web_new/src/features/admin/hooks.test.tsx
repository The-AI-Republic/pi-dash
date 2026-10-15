// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { InstanceBootstrap, InstancePatch, Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return { ...original, notifySuccess: vi.fn() };
});

import { notifySuccess } from "../../shared/shell/index.js";
import type { CloudEntrySessionState } from "./cloudEntry.js";
import {
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
} from "./generalSettings.js";
import {
  useAdminCsrfToken,
  useAdminSession,
  useCloudAuthProbe,
  useCloudEntryRetry,
  useCloudSignInUrl,
  useInstanceAdmins,
  useInstanceBootstrap,
  useUpdateGeneralSettings,
} from "./hooks.js";
import { adminKeys } from "./queries.js";

function stubTransport(routes: Record<string, { status: number; body: unknown }>): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    const route = routes[path] ?? { status: 404, body: { detail: "not found" } };
    return Promise.resolve({
      status: route.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(route.body)),
    });
  };
}

function renderProbe(Probe: () => React.ReactElement): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Probe />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.mocked(notifySuccess).mockClear();
  vi.restoreAllMocks();
});

function BootstrapProbe(): React.ReactElement {
  const query = useInstanceBootstrap();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.instance.is_setup_done ? "set-up" : "first-run"}</div>;
}

describe("useInstanceBootstrap", () => {
  const envelope = (setupDone: boolean) => ({
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
  });

  it("reads the setup flag through the app client", async () => {
    createAppClient("https://api.test", stubTransport({ "/api/instances/": { status: 200, body: envelope(true) } }));
    renderProbe(BootstrapProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("set-up"));
  });

  it("reports the first-run branch", async () => {
    createAppClient("https://api.test", stubTransport({ "/api/instances/": { status: 200, body: envelope(false) } }));
    renderProbe(BootstrapProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("first-run"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(BootstrapProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function CsrfProbe(): React.ReactElement {
  const query = useAdminCsrfToken();
  return <div data-testid="probe">{query.data === undefined ? "loading" : query.data.csrf_token}</div>;
}

describe("useAdminCsrfToken", () => {
  it("reads the forgery token through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/auth/get-csrf-token/": { status: 200, body: { csrf_token: "token-1" } } })
    );
    renderProbe(CsrfProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("token-1"));
  });
});

function SessionProbe(): React.ReactElement {
  const query = useAdminSession();
  if (query.isPending) return <div data-testid="probe">resolving</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.kind}</div>;
}

describe("useAdminSession", () => {
  const adminMe = {
    id: "22222222-2222-4222-8222-222222222222",
    avatar: "https://cdn.test/avatar.png",
    avatar_url: null,
    cover_image: null,
    date_joined: "2026-01-01T00:00:00Z",
    display_name: "Admin",
    email: "admin@example.com",
    first_name: "Admin",
    last_name: "User",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    user_timezone: "UTC",
    username: "admin",
    is_password_autoset: false,
  };

  it("reads the admin verdict through the app client", async () => {
    createAppClient("https://api.test", stubTransport({ "/api/instances/admins/me/": { status: 200, body: adminMe } }));
    renderProbe(SessionProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("admin"));
  });

  it("maps a rejected check to the anonymous branch", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/admins/me/": { status: 401, body: { detail: "anonymous" } } })
    );
    renderProbe(SessionProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("anonymous"));
  });

  it("surfaces a failed check instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(SessionProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function ProbeStatusProbe({ session }: { session: CloudEntrySessionState }): React.ReactElement {
  const query = useCloudAuthProbe(session);
  return <div data-testid={`probe-${session}`}>{query.fetchStatus}</div>;
}

function ProbeGatingProbe(): React.ReactElement {
  const states: CloudEntrySessionState[] = ["pending", "admin", "anonymous", "forbidden", "error"];
  return (
    <>
      {states.map((session) => (
        <ProbeStatusProbe key={session} session={session} />
      ))}
    </>
  );
}

describe("useCloudAuthProbe", () => {
  const probeUser = {
    id: "user-1",
    email: "user@example.com",
    display_name: "User",
    first_name: "User",
    last_name: "Person",
    avatar: "https://cdn.test/avatar.png",
  };

  it("fires exactly on a forbidden verdict", async () => {
    const seen: string[] = [];
    createAppClient("https://api.test", ((url: string) => {
      seen.push(new URL(url, "https://api.test").pathname);
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(probeUser)),
      });
    }) as Transport);
    renderProbe(ProbeGatingProbe);
    await waitFor(() => expect(screen.getByTestId("probe-forbidden")).toHaveTextContent("idle"));
    expect(seen).toEqual(["/api/auth/me/"]);
    for (const session of ["pending", "admin", "anonymous", "error"]) {
      expect(screen.getByTestId(`probe-${session}`)).toHaveTextContent("idle");
    }
  });
});

function SignInUrlProbe(): React.ReactElement {
  return <div data-testid="probe">{useCloudSignInUrl("/god-mode/loop/3/?tab=targets")}</div>;
}

describe("useCloudSignInUrl", () => {
  it("builds the sign-in entry against the app base with the encoded return", () => {
    createAppClient("https://api.test/", stubTransport({}));
    render(<SignInUrlProbe />);
    expect(screen.getByTestId("probe")).toHaveTextContent(
      "https://api.test/api/auth/sign-in/?return_to=%2Fgod-mode%2Floop%2F3%2F%3Ftab%3Dtargets"
    );
  });
});

function RetryProbe(): React.ReactElement {
  const retry = useCloudEntryRetry();
  return (
    <button data-testid="probe" type="button" onClick={retry}>
      retry
    </button>
  );
}

describe("useCloudEntryRetry", () => {
  it("resets the check and the probe verdicts to pending", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.session, { kind: "anonymous" });
    queryClient.setQueryData(adminKeys.cloudProbe, { kind: "inconclusive" });
    render(
      <QueryClientProvider client={queryClient}>
        <RetryProbe />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByTestId("probe"));
    await waitFor(() => {
      expect(queryClient.getQueryState(adminKeys.session)?.data).toBeUndefined();
      expect(queryClient.getQueryState(adminKeys.cloudProbe)?.data).toBeUndefined();
    });
  });
});

function AdminsProbe(): React.ReactElement {
  const query = useInstanceAdmins();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.map((entry) => entry.user_detail.email).join(",")}</div>;
}

describe("useInstanceAdmins", () => {
  const adminRow = {
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    user: "44444444-4444-4444-8444-444444444444",
    instance: "55555555-5555-4555-8555-555555555555",
    role: 1,
    is_verified: true,
    user_detail: {
      id: "44444444-4444-4444-8444-444444444444",
      first_name: "Ada",
      last_name: "Admin",
      avatar: "",
      avatar_url: null,
      is_bot: false,
      display_name: "Ada Admin",
      email: "ada@example.com",
      last_login_medium: "password",
    },
  };

  it("lists the admins through the app client", async () => {
    createAppClient("https://api.test", stubTransport({ "/api/instances/admins/": { status: 200, body: [adminRow] } }));
    renderProbe(AdminsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("ada@example.com"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(AdminsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function SaveProbe({ patch }: { patch: InstancePatch }): React.ReactElement {
  const saving = useUpdateGeneralSettings();
  return (
    <button data-testid="probe" data-state={saving.status} type="button" onClick={() => saving.mutate(patch)}>
      save
    </button>
  );
}

/** The stored row the PATCH answers (parsed strictly, so fully shaped). */
function storedRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "55555555-5555-4555-8555-555555555555",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    instance_name: "Renamed",
    whitelist_emails: null,
    instance_id: "instance-1",
    current_version: "1.0.0",
    latest_version: null,
    edition: "oss",
    domain: "example.test",
    last_checked_at: "2026-01-01T00:00:00Z",
    namespace: null,
    is_telemetry_enabled: false,
    is_support_required: false,
    is_setup_done: true,
    is_signup_screen_visited: true,
    is_verified: false,
    is_test: false,
    is_current_version_deprecated: false,
    ...overrides,
  };
}

describe("useUpdateGeneralSettings", () => {
  interface SeenWrite {
    method: string;
    path: string;
    body: unknown;
  }

  function renderSave(options: { patchStatus?: number; bootstrap?: InstanceBootstrap }): {
    queryClient: QueryClient;
    writes: SeenWrite[];
  } {
    const writes: SeenWrite[] = [];
    const transport: Transport = async (url: string, init: TransportInit) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/auth/get-csrf-token/") {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-1" })),
        };
      }
      if (path === "/api/instances/" && init.method === "PATCH") {
        writes.push({ method: init.method, path, body: JSON.parse(init.body ?? "{}") });
        return {
          status: options.patchStatus ?? 200,
          url,
          headers: { get: () => null },
          text: () =>
            Promise.resolve(
              (options.patchStatus ?? 200) === 200 ? JSON.stringify(storedRow()) : JSON.stringify({ detail: "boom" })
            ),
        };
      }
      return {
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      };
    };
    createAppClient("https://api.test", transport);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    if (options.bootstrap !== undefined) queryClient.setQueryData(adminKeys.bootstrap, options.bootstrap);
    render(
      <QueryClientProvider client={queryClient}>
        <SaveProbe patch={{ instance_name: "Renamed", is_telemetry_enabled: false }} />
      </QueryClientProvider>
    );
    return { queryClient, writes };
  }

  function cachedBootstrap(): InstanceBootstrap {
    return {
      config: { enable_signup: true },
      instance: { ...storedRow({ instance_name: "Pi Dash", is_telemetry_enabled: true }), workspaces_exist: true },
    } as InstanceBootstrap;
  }

  it("saves name plus telemetry, merges the answer, and raises the notice", async () => {
    const { queryClient, writes } = renderSave({ bootstrap: cachedBootstrap() });
    fireEvent.click(screen.getByTestId("probe"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveAttribute("data-state", "success"));
    expect(writes).toEqual([
      { method: "PATCH", path: "/api/instances/", body: { instance_name: "Renamed", is_telemetry_enabled: false } },
    ]);
    const cached = queryClient.getQueryData<InstanceBootstrap>(adminKeys.bootstrap);
    expect(cached?.instance.instance_name).toBe("Renamed");
    expect(cached?.instance.is_telemetry_enabled).toBe(false);
    expect(cached?.instance.workspaces_exist).toBe(true);
    expect(notifySuccess).toHaveBeenCalledWith(GENERAL_SAVE_SUCCESS_TITLE, GENERAL_SAVE_SUCCESS_MESSAGE);
    expect(notifySuccess).toHaveBeenCalledTimes(1);
  });

  it("still raises the notice when no bootstrap is cached", async () => {
    const { queryClient, writes } = renderSave({});
    fireEvent.click(screen.getByTestId("probe"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveAttribute("data-state", "success"));
    expect(writes).toHaveLength(1);
    expect(queryClient.getQueryData(adminKeys.bootstrap)).toBeUndefined();
    expect(notifySuccess).toHaveBeenCalledWith(GENERAL_SAVE_SUCCESS_TITLE, GENERAL_SAVE_SUCCESS_MESSAGE);
  });

  it("logs the failure and shows no notice (ADM-017 log-only quirk)", async () => {
    const errors: unknown[][] = [];
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });
    renderSave({ bootstrap: cachedBootstrap(), patchStatus: 500 });
    fireEvent.click(screen.getByTestId("probe"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveAttribute("data-state", "error"));
    expect(errors).toHaveLength(1);
    expect(errors[0]?.[0]).toBe(GENERAL_SAVE_FAILURE_LOG);
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});
