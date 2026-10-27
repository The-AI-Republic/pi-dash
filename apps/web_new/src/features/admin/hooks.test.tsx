// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { InstanceBootstrap, InstancePatch, LoopJobList, Transport, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toastHarness = vi.hoisted(() => ({ close: vi.fn(), progressId: "progress-toast" }));

const { notifyErrorMock } = vi.hoisted(() => ({ notifyErrorMock: vi.fn() }));

vi.mock("../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../shared/shell/index.js")>();
  return {
    ...actual,
    getToastManager: () => ({ close: toastHarness.close }),
    notifyError: notifyErrorMock,
    notifySuccess: vi.fn(),
  };
});

vi.mock("@pidash/kit", () => ({ showToast: vi.fn(() => toastHarness.progressId) }));

import { showToast } from "@pidash/kit";
import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { getToastManager, notifyError, notifySuccess } from "../../shared/shell/index.js";
import type { CloudEntrySessionState } from "./cloudEntry.js";
import {
  SMTP_ENABLED_KEY,
  SMTP_FROM_KEY,
  SMTP_HOST_KEY,
  SMTP_PASSWORD_KEY,
  SMTP_PORT_KEY,
  SMTP_SSL_KEY,
  SMTP_TLS_KEY,
  SMTP_USER_KEY,
} from "./emailConfig.js";
import {
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
} from "./generalSettings.js";
import {
  CONFIG_SAVE_FAILED_LOG,
  createInstanceWorkspaceMutationOptions,
  createLoopJobMutationOptions,
  deleteLoopJobMutationOptions,
  toggleLoopJobMutationOptions,
  configValueMap,
  EMAIL_DISABLE_ERROR_LOG,
  EMAIL_DISABLE_FAILED_MESSAGE,
  EMAIL_DISABLE_FAILED_TITLE,
  EMAIL_DISABLED_TOAST_MESSAGE,
  EMAIL_DISABLED_TOAST_TITLE,
  SMTP_SAVE_ERROR_LOG,
  SMTP_SAVE_TOAST_MESSAGE,
  SMTP_SAVE_TOAST_TITLE,
  updateLoopJobMutationOptions,
  useAdminCsrfToken,
  useAdminSession,
  useCheckEmailCredentials,
  useCloudAuthProbe,
  useCloudEntryRetry,
  useCloudSignInUrl,
  useCreateInstanceWorkspace,
  useDeleteLoopJob,
  useDisableEmailFeature,
  useInstanceAdmins,
  useInstanceBootstrap,
  useInstanceConfigurations,
  useInstanceRunners,
  useInstanceWorkspaces,
  useLoopJobDetail,
  useLoopJobs,
  useLoopJobTargets,
  useRestrictWorkspaceCreation,
  useSaveSmtpConfig,
  useUpdateGeneralSettings,
  useUpdateInstanceConfigurations,
} from "./hooks.js";
import { LOOP_TOGGLE_FAILURE_MESSAGE, LOOP_TOGGLE_FAILURE_TITLE } from "./loopJobs.js";
import { LOOP_JOB_DELETE_FAILED_MESSAGE, LOOP_JOB_DELETE_FAILED_TITLE } from "./loopJobDetail.js";
import { LOOP_JOB_SAVE_FAILED_TITLE } from "./loopJobModal.js";
import { adminKeys } from "./queries.js";
import {
  WORKSPACE_CREATE_FAILURE_TITLE,
  WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_MESSAGE,
  WORKSPACE_CREATE_SUCCESS_TITLE,
  WorkspaceSlugProbeError,
  WorkspaceSlugTakenError,
} from "./workspaceCreate.js";

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

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.mocked(notifySuccess).mockClear();
  vi.restoreAllMocks();
  vi.clearAllMocks();
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

function smtpRow(key: string, value: string | null) {
  return {
    id: "00000000-0000-4000-8000-000000000001",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "smtp",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

const STORED_SMTP = [
  smtpRow(SMTP_HOST_KEY, "smtp.example.com"),
  smtpRow(SMTP_PORT_KEY, "587"),
  smtpRow(SMTP_FROM_KEY, "team@example.com"),
  smtpRow(SMTP_USER_KEY, "mailer"),
  smtpRow(SMTP_PASSWORD_KEY, "secret"),
  smtpRow(SMTP_TLS_KEY, "1"),
  smtpRow(SMTP_SSL_KEY, "0"),
  smtpRow(SMTP_ENABLED_KEY, "1"),
];

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function recordingTransport(
  routes: Record<string, { status: number; body: unknown }>,
  calls: Array<{ method: string | undefined; path: string; body: unknown }>
): Transport {
  return (url, options) => {
    const path = new URL(url, "https://api.test").pathname;
    calls.push({
      method: options?.method,
      path,
      body: options?.body === undefined ? undefined : JSON.parse(options.body as string),
    });
    const route = routes[path] ?? { status: 404, body: { detail: "not found" } };
    return Promise.resolve({
      status: route.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(route.body)),
    });
  };
}

function renderWithClient(Probe: () => React.ReactElement): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <Probe />
    </QueryClientProvider>
  );
  return queryClient;
}

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

function configRow(key: string, value: string | null) {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "AUTHENTICATION",
    is_encrypted: false,
    source: "db",
    is_managed: false,
  };
}

function ConfigurationsProbe(): React.ReactElement {
  const query = useInstanceConfigurations();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  const signup = query.data.find((row) => row.key === "ENABLE_SIGNUP")?.value ?? "missing";
  return <div data-testid="probe">signup:{signup}</div>;
}

describe("useInstanceConfigurations", () => {
  it("reads the shared key map through the client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/configurations/": { status: 200, body: [configRow("ENABLE_SIGNUP", "1")] } })
    );
    renderProbe(ConfigurationsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("signup:1"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(ConfigurationsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function ConfigurationsSaveProbe({ patch }: { patch: Record<string, string> }): React.ReactElement {
  const save = useUpdateInstanceConfigurations();
  return (
    <button data-testid="probe" type="button" onClick={() => save.mutate(patch)}>
      {save.isPending ? "saving" : "save"}
    </button>
  );
}

function stubSavingTransport(args: { patchStatus: number; patchBody: unknown; calls: string[] }): Transport {
  return (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    args.calls.push(`${method} ${new URL(url, "https://api.test").pathname}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf" })),
      };
    }
    if (url.endsWith("/api/instances/configurations/") && method === "PATCH") {
      return {
        status: args.patchStatus,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(args.patchBody)),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: () => Promise.resolve("{}") };
  }) as Transport;
}

describe("useUpdateInstanceConfigurations", () => {
  it("patches one key and merges the answered rows into the cache", async () => {
    const calls: string[] = [];
    createAppClient(
      "https://api.test",
      stubSavingTransport({ patchStatus: 200, patchBody: [configRow("ENABLE_SIGNUP", "0")], calls })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.configurations, [
      configRow("ENABLE_SIGNUP", "1"),
      configRow("ENABLE_MAGIC_LINK_LOGIN", "1"),
    ]);
    const errored = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      render(
        <QueryClientProvider client={queryClient}>
          <ConfigurationsSaveProbe patch={{ ENABLE_SIGNUP: "0" }} />
        </QueryClientProvider>
      );
      fireEvent.click(screen.getByTestId("probe"));
      await waitFor(() => {
        const cached = queryClient.getQueryData(adminKeys.configurations) as Array<{ key: string; value: string }>;
        expect(cached.find((row) => row.key === "ENABLE_SIGNUP")?.value).toBe("0");
      });
      const cached = queryClient.getQueryData(adminKeys.configurations) as Array<{ key: string; value: string }>;
      expect(cached.find((row) => row.key === "ENABLE_MAGIC_LINK_LOGIN")?.value).toBe("1");
      expect(calls).toContain("PATCH /api/instances/configurations/");
      expect(errored).not.toHaveBeenCalled();
    } finally {
      errored.mockRestore();
    }
  });

  it("logs the pinned line and keeps the cache on failure", async () => {
    const calls: string[] = [];
    createAppClient(
      "https://api.test",
      stubSavingTransport({ patchStatus: 500, patchBody: { detail: "boom" }, calls })
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(adminKeys.configurations, [configRow("ENABLE_SIGNUP", "1")]);
    const errored = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      render(
        <QueryClientProvider client={queryClient}>
          <ConfigurationsSaveProbe patch={{ ENABLE_SIGNUP: "0" }} />
        </QueryClientProvider>
      );
      fireEvent.click(screen.getByTestId("probe"));
      await waitFor(() => expect(errored).toHaveBeenCalledWith(CONFIG_SAVE_FAILED_LOG, expect.anything()));
      const cached = queryClient.getQueryData(adminKeys.configurations) as Array<{ key: string; value: string }>;
      expect(cached.find((row) => row.key === "ENABLE_SIGNUP")?.value).toBe("1");
    } finally {
      errored.mockRestore();
    }
  });
});

function loopJob(overrides: Partial<LoopJobList[number]> = {}): LoopJobList[number] {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    slug: "probe-job",
    name: "Probe job",
    public_name: "Probe public",
    public_description: "Probe description",
    prompt: "Probe prompt",
    min_role: 15,
    enabled: true,
    is_builtin: false,
    dtstart: null,
    rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    tzid: "UTC",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function LoopJobsProbe(): React.ReactElement {
  const query = useLoopJobs();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.map((job) => job.slug).join(",")}</div>;
}

describe("useLoopJobs", () => {
  it("reads the job list through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/loop/jobs/": { status: 200, body: [loopJob(), loopJob({ slug: "probe-two" })] } })
    );
    renderProbe(LoopJobsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("probe-job,probe-two"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(LoopJobsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

describe("toggleLoopJobMutationOptions", () => {
  function stubClient() {
    const sendJson = vi.fn(async () => ({ data: {}, status: 200, url: "https://api.test/" }));
    const client = { sendJson, parse: (_schema: unknown, data: unknown) => data } as never;
    return { client, sendJson };
  }

  it("patches only the enabled flag through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = toggleLoopJobMutationOptions(client, new QueryClient());
    await options.mutationFn?.({ id: "job-1", enabled: false }, {} as never);
    expect(sendJson).toHaveBeenCalledWith("PATCH", "/api/instances/loop/jobs/job-1/", { json: { enabled: false } });
  });

  it("flips the one job optimistically and leaves the others alone", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(adminKeys.loopJobs, [
      loopJob({ id: "job-1", enabled: true }),
      loopJob({ id: "job-2", enabled: true }),
    ]);
    const options = toggleLoopJobMutationOptions(stubClient().client, queryClient);
    await options.onMutate?.({ id: "job-2", enabled: false }, {} as never);
    expect((queryClient.getQueryData(adminKeys.loopJobs) as LoopJobList).map((job) => job.enabled)).toEqual([
      true,
      false,
    ]);
  });

  it("tolerates an empty cache on mutate", async () => {
    const queryClient = new QueryClient();
    const options = toggleLoopJobMutationOptions(stubClient().client, queryClient);
    const context = await options.onMutate?.({ id: "job-1", enabled: false }, {} as never);
    expect(queryClient.getQueryData(adminKeys.loopJobs)).toBeUndefined();
    options.onError?.(new Error("denied"), { id: "job-1", enabled: false }, context, {} as never);
    expect(queryClient.getQueryData(adminKeys.loopJobs)).toBeUndefined();
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_TOGGLE_FAILURE_TITLE, LOOP_TOGGLE_FAILURE_MESSAGE);
  });

  it("rolls back and toasts the pinned notice on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(adminKeys.loopJobs, [loopJob({ id: "job-1", enabled: true })]);
    const options = toggleLoopJobMutationOptions(stubClient().client, queryClient);
    const context = await options.onMutate?.({ id: "job-1", enabled: false }, {} as never);
    expect((queryClient.getQueryData(adminKeys.loopJobs) as LoopJobList)[0]?.enabled).toBe(false);
    options.onError?.(new Error("denied"), { id: "job-1", enabled: false }, context, {} as never);
    expect((queryClient.getQueryData(adminKeys.loopJobs) as LoopJobList)[0]?.enabled).toBe(true);
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_TOGGLE_FAILURE_TITLE, LOOP_TOGGLE_FAILURE_MESSAGE);
  });

  it("toasts even with no optimistic context to roll back", () => {
    const options = toggleLoopJobMutationOptions(stubClient().client, new QueryClient());
    options.onError?.(new Error("denied"), { id: "job-1", enabled: false }, undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_TOGGLE_FAILURE_TITLE, LOOP_TOGGLE_FAILURE_MESSAGE);
  });

  it("stays silent on success", () => {
    const options = toggleLoopJobMutationOptions(stubClient().client, new QueryClient());
    expect(options.onSuccess).toBeUndefined();
  });

  it("revalidates the list once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = toggleLoopJobMutationOptions(stubClient().client, queryClient);
    options.onSettled?.(undefined, null, { id: "job-1", enabled: false }, undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.loopJobs });
  });
});

describe("configValueMap", () => {
  it("maps rows by key", () => {
    expect(configValueMap(STORED_SMTP as never)).toEqual({
      [SMTP_HOST_KEY]: "smtp.example.com",
      [SMTP_PORT_KEY]: "587",
      [SMTP_FROM_KEY]: "team@example.com",
      [SMTP_USER_KEY]: "mailer",
      [SMTP_PASSWORD_KEY]: "secret",
      [SMTP_TLS_KEY]: "1",
      [SMTP_SSL_KEY]: "0",
      [SMTP_ENABLED_KEY]: "1",
    });
  });

  it("maps a missing read to an empty map", () => {
    expect(configValueMap(undefined)).toEqual({});
  });
});

function SmtpSaveProbe({ patch }: { patch: Record<string, string> }): React.ReactElement {
  const save = useSaveSmtpConfig();
  return (
    <>
      <button type="button" data-testid="fire" onClick={() => save.mutate(patch)}>
        fire
      </button>
      <div data-testid="probe">{save.isPending ? "saving" : save.isSuccess ? "saved" : "idle"}</div>
    </>
  );
}

describe("useSaveSmtpConfig", () => {
  const patch = { [SMTP_HOST_KEY]: "smtp.example.com", [SMTP_ENABLED_KEY]: "1" };

  it("persists the patch, merges the answered rows, and toasts success", async () => {
    const calls: Array<{ method: string | undefined; path: string; body: unknown }> = [];
    createAppClient(
      "https://api.test",
      recordingTransport(
        {
          "/api/instances/configurations/": {
            status: 200,
            body: [smtpRow(SMTP_HOST_KEY, "smtp.example.com"), smtpRow(SMTP_ENABLED_KEY, "1")],
          },
        },
        calls
      )
    );
    const queryClient = renderWithClient(() => <SmtpSaveProbe patch={patch} />);
    queryClient.setQueryData(adminKeys.configurations, STORED_SMTP);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("saved"));
    expect(calls.filter((call) => call.method === "PATCH")).toEqual([
      { method: "PATCH", path: "/api/instances/configurations/", body: patch },
    ]);
    const cached = queryClient.getQueryData(adminKeys.configurations) as Array<{ key: string; value: string }>;
    expect(cached.find((row) => row.key === SMTP_HOST_KEY)?.value).toBe("smtp.example.com");
    expect(cached.find((row) => row.key === SMTP_TLS_KEY)?.value).toBe("1");
    expect(notifySuccess).toHaveBeenCalledWith(SMTP_SAVE_TOAST_TITLE, SMTP_SAVE_TOAST_MESSAGE);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("logs a failed save to the console with no notice (ADM-019 quirk)", async () => {
    const errors: unknown[][] = [];
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/configurations/": { status: 500, body: { detail: "gone" } } })
    );
    const queryClient = renderWithClient(() => <SmtpSaveProbe patch={patch} />);
    queryClient.setQueryData(adminKeys.configurations, STORED_SMTP);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(errors.length).toBe(1));
    expect(errors[0]?.[0]).toBe(SMTP_SAVE_ERROR_LOG);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(notifyError).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(adminKeys.configurations)).toEqual(STORED_SMTP);
  });
});

function DisableProbe(): React.ReactElement {
  const disable = useDisableEmailFeature();
  return (
    <>
      <button type="button" data-testid="fire" onClick={() => disable.mutate(undefined)}>
        fire
      </button>
      <div data-testid="probe">{disable.isPending ? "disabling" : disable.isSuccess ? "disabled" : "idle"}</div>
    </>
  );
}

function cachedSmtp(queryClient: QueryClient): Record<string, string | null> {
  const cached = queryClient.getQueryData(adminKeys.configurations) as Array<{
    key: string;
    value: string | null;
  }>;
  return Object.fromEntries(cached.map((row) => [row.key, row.value]));
}

describe("useDisableEmailFeature", () => {
  it("blanks the mail values optimistically while keeping the security pair", async () => {
    const gate = deferred<{ status: number; url: string; headers: { get: () => null }; text: () => Promise<string> }>();
    createAppClient("https://api.test", ((url: string) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/api/instances/configurations/disable-email-feature/") return gate.promise;
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    }) as Transport);
    const queryClient = renderWithClient(DisableProbe);
    queryClient.setQueryData(adminKeys.configurations, STORED_SMTP);
    fireEvent.click(screen.getByTestId("fire"));
    // The blanking lands before the call resolves.
    await waitFor(() => expect(cachedSmtp(queryClient)[SMTP_ENABLED_KEY]).toBe("0"));
    expect(cachedSmtp(queryClient)).toEqual({
      [SMTP_HOST_KEY]: "",
      [SMTP_PORT_KEY]: "",
      [SMTP_FROM_KEY]: "",
      [SMTP_USER_KEY]: "",
      [SMTP_PASSWORD_KEY]: "",
      [SMTP_TLS_KEY]: "1",
      [SMTP_SSL_KEY]: "0",
      [SMTP_ENABLED_KEY]: "0",
    });
    gate.resolve({
      status: 200,
      url: "https://api.test/api/instances/configurations/disable-email-feature/",
      headers: { get: () => null },
      text: () => Promise.resolve(""),
    });
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("disabled"));
    expect(notifySuccess).toHaveBeenCalledWith(EMAIL_DISABLED_TOAST_TITLE, EMAIL_DISABLED_TOAST_MESSAGE);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("restores the values and toasts the error when the call fails (NEWFRONT-476 intent)", async () => {
    const errors: unknown[][] = [];
    vi.spyOn(console, "error").mockImplementation((...args: unknown[]) => {
      errors.push(args);
    });
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/configurations/disable-email-feature/": {
          status: 400,
          body: { error: "Failed to disable email configuration" },
        },
      })
    );
    const queryClient = renderWithClient(DisableProbe);
    queryClient.setQueryData(adminKeys.configurations, STORED_SMTP);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(EMAIL_DISABLE_FAILED_TITLE, EMAIL_DISABLE_FAILED_MESSAGE);
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(errors[0]?.[0]).toBe(EMAIL_DISABLE_ERROR_LOG);
    expect(queryClient.getQueryData(adminKeys.configurations)).toEqual(STORED_SMTP);
  });

  it("still toasts the error when nothing was cached to restore", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    createAppClient("https://api.test", stubTransport({}));
    const queryClient = renderWithClient(DisableProbe);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(EMAIL_DISABLE_FAILED_TITLE, EMAIL_DISABLE_FAILED_MESSAGE);
    expect(queryClient.getQueryData(adminKeys.configurations)).toEqual([]);
  });

  it("still toasts the error when the optimistic update itself throws", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/configurations/disable-email-feature/": { status: 200, body: null },
      })
    );
    const queryClient = renderWithClient(DisableProbe);
    queryClient.setQueryData(adminKeys.configurations, STORED_SMTP);
    vi.spyOn(queryClient, "cancelQueries").mockRejectedValueOnce(new Error("cancel gone"));
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(notifyError).toHaveBeenCalledWith(EMAIL_DISABLE_FAILED_TITLE, EMAIL_DISABLE_FAILED_MESSAGE);
  });
});

function CheckProbe({ receiver }: { receiver: string }): React.ReactElement {
  const check = useCheckEmailCredentials();
  return (
    <>
      <button
        type="button"
        data-testid="fire"
        onClick={() =>
          check.mutate({
            receiverEmail: receiver,
            candidate: { host: "smtp.example.com", port: "587", from: "team@example.com", security: "tls" },
          })
        }
      >
        fire
      </button>
      <div data-testid="probe">
        {check.isPending ? "sending" : check.isSuccess ? check.data.message : check.isError ? "failed" : "idle"}
      </div>
    </>
  );
}

describe("useCheckEmailCredentials", () => {
  it("posts the receiver with the candidate settings and resolves the confirmation", async () => {
    const calls: Array<{ method: string | undefined; path: string; body: unknown }> = [];
    createAppClient(
      "https://api.test",
      recordingTransport(
        { "/api/instances/email-credentials-check/": { status: 200, body: { message: "Email successfully sent." } } },
        calls
      )
    );
    renderWithClient(() => <CheckProbe receiver="admin@example.com" />);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("Email successfully sent."));
    expect(calls.filter((call) => call.method === "POST")).toEqual([
      {
        method: "POST",
        path: "/api/instances/email-credentials-check/",
        body: {
          receiver_email: "admin@example.com",
          EMAIL_HOST: "smtp.example.com",
          EMAIL_PORT: "587",
          EMAIL_FROM: "team@example.com",
          EMAIL_USE_TLS: "1",
          EMAIL_USE_SSL: "0",
        },
      },
    ]);
  });

  it("surfaces the server failure to the caller", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/email-credentials-check/": { status: 400, body: { error: "Receiver email is required" } },
      })
    );
    renderWithClient(() => <CheckProbe receiver="" />);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function AiConfigurationsProbe(): React.ReactElement {
  const query = useInstanceConfigurations();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.map((row) => row.key).join(",")}</div>;
}

describe("useInstanceConfigurations", () => {
  const rows = [
    {
      id: "11111111-1111-4111-8111-111111111111",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      key: "LLM_MODEL",
      value: "gpt-4o",
      category: "ai",
      is_encrypted: false,
      source: "db",
      is_managed: true,
    },
  ];

  it("reads the shared key map through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/api/instances/configurations/": { status: 200, body: rows } })
    );
    renderProbe(AiConfigurationsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("LLM_MODEL"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(AiConfigurationsProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

describe("createLoopJobMutationOptions", () => {
  function stubClient() {
    const sendJson = vi.fn(async () => ({ data: {}, status: 201, url: "https://api.test/" }));
    const client = { sendJson, parse: (_schema: unknown, data: unknown) => data } as never;
    return { client, sendJson };
  }

  function createInput() {
    return {
      slug: "probe-job",
      name: "Probe job",
      public_name: "Probe public",
      public_description: "Probe description",
      prompt: "Probe prompt",
      min_role: 20,
      rrule: "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
      tzid: "UTC",
    };
  }

  it("posts the create body through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = createLoopJobMutationOptions(client, new QueryClient());
    await options.mutationFn?.(createInput(), {} as never);
    expect(sendJson).toHaveBeenCalledWith("POST", "/api/instances/loop/jobs/", { json: createInput() });
  });

  it("toasts the server text on failure", () => {
    const options = createLoopJobMutationOptions(stubClient().client, new QueryClient());
    options.onError?.(new Error("parity-probe-failure"), createInput(), undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_JOB_SAVE_FAILED_TITLE, "parity-probe-failure");
  });

  it("falls back when the failure carries no text", () => {
    const options = createLoopJobMutationOptions(stubClient().client, new QueryClient());
    options.onError?.(null, createInput(), undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_JOB_SAVE_FAILED_TITLE, "Check the fields and try again.");
  });

  it("stays silent on success", () => {
    const options = createLoopJobMutationOptions(stubClient().client, new QueryClient());
    expect(options.onSuccess).toBeUndefined();
  });

  it("revalidates the list once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = createLoopJobMutationOptions(stubClient().client, queryClient);
    options.onSettled?.(undefined, null, createInput(), undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.loopJobs });
  });
});

describe("updateLoopJobMutationOptions", () => {
  function stubClient() {
    const sendJson = vi.fn(async () => ({ data: {}, status: 200, url: "https://api.test/" }));
    const client = { sendJson, parse: (_schema: unknown, data: unknown) => data } as never;
    return { client, sendJson };
  }

  function update() {
    return { id: "job-1", patch: { name: "Renamed" } };
  }

  it("patches the job through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = updateLoopJobMutationOptions(client, new QueryClient());
    await options.mutationFn?.(update(), {} as never);
    expect(sendJson).toHaveBeenCalledWith("PATCH", "/api/instances/loop/jobs/job-1/", {
      json: { name: "Renamed" },
    });
  });

  it("toasts the server text on failure", () => {
    const options = updateLoopJobMutationOptions(stubClient().client, new QueryClient());
    options.onError?.(new Error("parity-probe-failure"), update(), undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_JOB_SAVE_FAILED_TITLE, "parity-probe-failure");
  });

  it("stays silent on success", () => {
    const options = updateLoopJobMutationOptions(stubClient().client, new QueryClient());
    expect(options.onSuccess).toBeUndefined();
  });

  it("revalidates the list and the edited job once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = updateLoopJobMutationOptions(stubClient().client, queryClient);
    options.onSettled?.(undefined, null, update(), undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.loopJobs });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.loopJobDetail("job-1") });
  });
});

function workspaceRow(slug: string, name: string): Record<string, unknown> {
  return {
    id: "55555555-5555-4555-8555-555555555555",
    name,
    slug,
    logo: null,
    logo_asset: null,
    logo_url: null,
    owner: {
      id: "66666666-6666-4666-8666-666666666666",
      email: "owner@example.com",
      first_name: "Ow",
      last_name: "Ner",
    },
    organization_size: "2-10",
    timezone: "UTC",
    background_color: "#ffffff",
    total_projects: 2,
    total_members: 3,
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };
}

function workspacePage(results: Record<string, unknown>[], nextCursor: string): Record<string, unknown> {
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: results.length,
    next_cursor: nextCursor,
    prev_cursor: "",
    next_page_results: nextCursor !== "",
    prev_page_results: false,
    count: results.length,
    total_results: results.length,
    results,
  };
}

function ConfigProbe(): React.ReactElement {
  const query = useInstanceConfigurations();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return (
    <div data-testid="probe">
      {query.data.find((row) => row.key === "DISABLE_WORKSPACE_CREATION")?.value ?? "absent"}
    </div>
  );
}

describe("useInstanceConfigurations", () => {
  it("reads the shared key map through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/configurations/": { status: 200, body: [configRow("DISABLE_WORKSPACE_CREATION", "0")] },
      })
    );
    renderProbe(ConfigProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("0"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(ConfigProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function WorkspacesProbe(): React.ReactElement {
  const query = useInstanceWorkspaces();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  const names = query.data.pages.flatMap((page) => page.results.map((row) => row.name)).join(",");
  return (
    <div>
      <div data-testid="probe">{names}</div>
      <div data-testid="more">{query.hasNextPage ? "more" : "done"}</div>
      <button type="button" data-testid="next" onClick={() => void query.fetchNextPage()}>
        next
      </button>
    </div>
  );
}

describe("useInstanceWorkspaces", () => {
  it("appends the next page through the cursor without refetching the first", async () => {
    const seen: string[] = [];
    const transport: Transport = (url) => {
      seen.push(url);
      const cursor = new URL(url, "https://api.test").searchParams.get("cursor");
      const body =
        cursor === null
          ? workspacePage([workspaceRow("acme", "Acme")], "cursor-2")
          : workspacePage([workspaceRow("globex", "Globex")], "");
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(body)),
      });
    };
    createAppClient("https://api.test", transport);
    renderProbe(WorkspacesProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("Acme"));
    expect(screen.getByTestId("more")).toHaveTextContent("more");
    fireEvent.click(screen.getByTestId("next"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("Acme,Globex"));
    expect(screen.getByTestId("more")).toHaveTextContent("done");
    expect(seen).toHaveLength(2);
    expect(seen[0]).not.toContain("cursor=");
    expect(seen[1]).toContain("cursor=cursor-2");
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(WorkspacesProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function RestrictProbe(): React.ReactElement {
  const mutation = useRestrictWorkspaceCreation();
  return (
    <div>
      <div data-testid="probe">
        {mutation.isPending ? "saving" : mutation.isError ? "failed" : mutation.isSuccess ? "saved" : "idle"}
      </div>
      <button type="button" data-testid="save-on" onClick={() => mutation.mutate(true)}>
        on
      </button>
      <button type="button" data-testid="save-off" onClick={() => mutation.mutate(false)}>
        off
      </button>
    </div>
  );
}

function renderRestrictProbe(queryClient: QueryClient): void {
  render(
    <QueryClientProvider client={queryClient}>
      <RestrictProbe />
    </QueryClientProvider>
  );
}

describe("useRestrictWorkspaceCreation", () => {
  const bodies: Array<{ method: string; body: string | undefined }> = [];

  function patchClient(status: number, answer: unknown): QueryClient {
    bodies.length = 0;
    const transport: Transport = (url, init) => {
      const path = new URL(url, "https://api.test").pathname;
      if (init.method === "PATCH" && path === "/api/instances/configurations/") {
        bodies.push({ method: init.method, body: init.body });
        return Promise.resolve({
          status,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify(answer)),
        });
      }
      return Promise.resolve({
        status: 404,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
      });
    };
    createAppClient("https://api.test", transport);
    return new QueryClient({ defaultOptions: { queries: { retry: false } } });
  }

  it("saves a single-key write with a progress notice resolving to success", async () => {
    const queryClient = patchClient(200, [configRow("DISABLE_WORKSPACE_CREATION", "1")]);
    queryClient.setQueryData(adminKeys.configurations, [
      configRow("DISABLE_WORKSPACE_CREATION", "0"),
      configRow("OTHER", "kept"),
    ]);
    renderRestrictProbe(queryClient);
    fireEvent.click(screen.getByTestId("save-on"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("saved"));
    expect(bodies).toEqual([{ method: "PATCH", body: JSON.stringify({ DISABLE_WORKSPACE_CREATION: "1" }) }]);
    const manager = getToastManager();
    expect(showToast).toHaveBeenCalledWith(manager, { title: "Saving configuration", timeout: 0 });
    expect(toastHarness.close).toHaveBeenCalledWith(toastHarness.progressId);
    expect(notifySuccess).toHaveBeenCalledWith("Success", "Configuration saved successfully");
    expect(notifyError).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(adminKeys.configurations)).toEqual([
      { ...configRow("DISABLE_WORKSPACE_CREATION", "1") },
      { ...configRow("OTHER", "kept") },
    ]);
  });

  it("encodes switching back to open as the zero flag", async () => {
    const queryClient = patchClient(200, [configRow("DISABLE_WORKSPACE_CREATION", "0")]);
    renderRestrictProbe(queryClient);
    fireEvent.click(screen.getByTestId("save-off"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("saved"));
    expect(bodies).toEqual([{ method: "PATCH", body: JSON.stringify({ DISABLE_WORKSPACE_CREATION: "0" }) }]);
    expect(notifySuccess).toHaveBeenCalledWith("Success", "Configuration saved successfully");
  });

  it("reports a failed save and keeps the stored value", async () => {
    const queryClient = patchClient(500, { detail: "boom" });
    queryClient.setQueryData(adminKeys.configurations, [configRow("DISABLE_WORKSPACE_CREATION", "0")]);
    renderRestrictProbe(queryClient);
    fireEvent.click(screen.getByTestId("save-on"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
    expect(bodies).toHaveLength(1);
    expect(toastHarness.close).toHaveBeenCalledWith(toastHarness.progressId);
    expect(notifyError).toHaveBeenCalledWith("Error", "Failed to save configuration");
    expect(notifySuccess).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(adminKeys.configurations)).toEqual([configRow("DISABLE_WORKSPACE_CREATION", "0")]);
  });
});

function LoopJobDetailProbe({ jobId }: { jobId: string }): React.ReactElement {
  const query = useLoopJobDetail(jobId);
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{`${query.data.slug}:${query.data.stats.target_count}`}</div>;
}

function LoopJobTargetsProbe({ jobId, skipReason }: { jobId: string; skipReason: string }): React.ReactElement {
  const query = useLoopJobTargets(jobId, skipReason);
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.results.map((target) => target.user_email).join(",")}</div>;
}

function DeleteLoopJobProbe(): React.ReactElement {
  const deletion = useDeleteLoopJob();
  return <div data-testid="probe">{deletion.isPending ? "deleting" : "ready"}</div>;
}

describe("useLoopJobDetail", () => {
  it("reads the job with its stats through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/loop/jobs/33333333-3333-4333-8333-333333333333/": {
          status: 200,
          body: { ...loopJob(), stats: { target_count: 2, completed: 0, failed: 0, skipped: 1 } },
        },
      })
    );
    renderProbe(() => LoopJobDetailProbe({ jobId: "33333333-3333-4333-8333-333333333333" }));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("probe-job:2"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(() => LoopJobDetailProbe({ jobId: "33333333-3333-4333-8333-333333333333" }));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

describe("useLoopJobTargets", () => {
  const target = {
    id: "44444444-4444-4434-8434-444444444444",
    workspace_slug: "probe-workspace",
    user_email: "member@example.test",
    next_run_at: "2026-02-03T04:05:06Z",
    last_skipped_at: null,
    last_skip_reason: "",
    last_run: null,
  };

  it("reads the targets through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/loop/jobs/33333333-3333-4333-8333-333333333333/targets/": {
          status: 200,
          body: { page: 1, results: [target] },
        },
      })
    );
    renderProbe(() => LoopJobTargetsProbe({ jobId: "33333333-3333-4333-8333-333333333333", skipReason: "" }));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("member@example.test"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(() => LoopJobTargetsProbe({ jobId: "33333333-3333-4333-8333-333333333333", skipReason: "min_role" }));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function InstanceRunnersProbe({ provisioning, page }: { provisioning: string; page: number }): React.ReactElement {
  const query = useInstanceRunners(provisioning, page);
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{`${query.data.total}:${query.data.results.map((row) => row.name).join(",")}`}</div>;
}

describe("useInstanceRunners", () => {
  const runner = {
    id: "55555555-5555-4535-8555-555555555555",
    name: "probe-runner",
    workspace_slug: "probe-workspace",
    owner_email: "member@example.test",
    host_label: "probe-runner",
    provisioning: "manual",
    status: "online",
    runner_version: "1.2.3",
    codex_version: null,
    last_heartbeat_at: null,
    created_at: "2026-02-03T04:05:06Z",
    revoked_at: null,
  };

  it("reads one runners page through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/runners/": {
          status: 200,
          body: { page: 1, total: 1, results: [runner] },
        },
      })
    );
    renderProbe(() => InstanceRunnersProbe({ provisioning: "", page: 1 }));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("1:probe-runner"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(() => InstanceRunnersProbe({ provisioning: "manual", page: 2 }));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

describe("deleteLoopJobMutationOptions", () => {
  function stubClient() {
    const remove = vi.fn(async () => ({ status: 204 }));
    const client = { delete: remove } as never;
    return { client, remove };
  }

  it("deletes the job through the contract", async () => {
    const { client, remove } = stubClient();
    const options = deleteLoopJobMutationOptions(client, new QueryClient());
    await expect(options.mutationFn?.("job-1", {} as never)).resolves.toBe(204);
    expect(remove).toHaveBeenCalledWith("/api/instances/loop/jobs/job-1/");
  });

  it("toasts the fixed notice on failure", () => {
    const options = deleteLoopJobMutationOptions(stubClient().client, new QueryClient());
    options.onError?.(new Error("denied"), "job-1", undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(LOOP_JOB_DELETE_FAILED_TITLE, LOOP_JOB_DELETE_FAILED_MESSAGE);
  });

  it("stays silent on success", () => {
    const options = deleteLoopJobMutationOptions(stubClient().client, new QueryClient());
    expect(options.onSuccess).toBeUndefined();
  });

  it("revalidates the list and the deleted job once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = deleteLoopJobMutationOptions(stubClient().client, queryClient);
    options.onSettled?.(204, null, "job-1", undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(2);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.loopJobs });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.loopJobDetail("job-1") });
  });

  it("wires the mutation through the hook", () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(DeleteLoopJobProbe);
    expect(screen.getByTestId("probe")).toHaveTextContent("ready");
  });
});

describe("createInstanceWorkspaceMutationOptions", () => {
  function stubWorkspaceClient(probe: { status: boolean } | Error, created: unknown = { id: "created-1" }) {
    const getJson = vi.fn(async () => {
      if (probe instanceof Error) throw probe;
      return probe;
    });
    const sendJson = vi.fn(async () => ({ data: created, status: 201, url: "https://api.test/" }));
    const client = { getJson, sendJson, parse: (_schema: unknown, data: unknown) => data } as never;
    return { client, getJson, sendJson };
  }

  function createInput() {
    return { name: "Good Name", slug: "good-slug-1", organization_size: "2-10" };
  }

  it("probes the slug first, then posts the create with no owner", async () => {
    const { client, getJson, sendJson } = stubWorkspaceClient({ status: true });
    const options = createInstanceWorkspaceMutationOptions(client, new QueryClient());
    await options.mutationFn?.(createInput(), {} as never);
    expect(getJson).toHaveBeenCalledWith("/api/instances/workspace-slug-check/", { query: { slug: "good-slug-1" } });
    expect(sendJson).toHaveBeenCalledWith("POST", "/api/instances/workspaces/", { json: createInput() });
    expect(getJson.mock.invocationCallOrder[0]).toBeLessThan(
      sendJson.mock.invocationCallOrder[0] ?? Number.MAX_SAFE_INTEGER
    );
  });

  it("raises taken with no create when the probe reports unavailable", async () => {
    const { client, sendJson } = stubWorkspaceClient({ status: false });
    const options = createInstanceWorkspaceMutationOptions(client, new QueryClient());
    await expect(options.mutationFn?.(createInput(), {} as never)).rejects.toBeInstanceOf(WorkspaceSlugTakenError);
    expect(sendJson).not.toHaveBeenCalled();
  });

  it("raises taken for a restricted slug even when the probe reports free", async () => {
    const { client, sendJson } = stubWorkspaceClient({ status: true });
    const options = createInstanceWorkspaceMutationOptions(client, new QueryClient());
    const input = { ...createInput(), slug: "god-mode" };
    await expect(options.mutationFn?.(input, {} as never)).rejects.toBeInstanceOf(WorkspaceSlugTakenError);
    expect(sendJson).not.toHaveBeenCalled();
  });

  it("raises the probe error with no create when the probe request fails", async () => {
    const { client, sendJson } = stubWorkspaceClient(new Error("parity-probe-down"));
    const options = createInstanceWorkspaceMutationOptions(client, new QueryClient());
    await expect(options.mutationFn?.(createInput(), {} as never)).rejects.toBeInstanceOf(WorkspaceSlugProbeError);
    expect(sendJson).not.toHaveBeenCalled();
  });

  it("stays silent on taken: the form renders the inline error", () => {
    const options = createInstanceWorkspaceMutationOptions(
      stubWorkspaceClient({ status: true }).client,
      new QueryClient()
    );
    options.onError?.(new WorkspaceSlugTakenError("taken-slug"), createInput(), undefined, {} as never);
    expect(notifyErrorMock).not.toHaveBeenCalled();
  });

  it("toasts the probe failure", () => {
    const options = createInstanceWorkspaceMutationOptions(
      stubWorkspaceClient({ status: true }).client,
      new QueryClient()
    );
    options.onError?.(new WorkspaceSlugProbeError("probe-slug"), createInput(), undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(
      WORKSPACE_CREATE_FAILURE_TITLE,
      WORKSPACE_CREATE_PROBE_FAILURE_MESSAGE
    );
  });

  it("toasts any other failure as a failed create", () => {
    const options = createInstanceWorkspaceMutationOptions(
      stubWorkspaceClient({ status: true }).client,
      new QueryClient()
    );
    options.onError?.(new Error("parity-create-down"), createInput(), undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith(WORKSPACE_CREATE_FAILURE_TITLE, WORKSPACE_CREATE_SAVE_FAILURE_MESSAGE);
  });

  it("revalidates the list and toasts success on create", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = createInstanceWorkspaceMutationOptions(stubWorkspaceClient({ status: true }).client, queryClient);
    options.onSuccess?.({ id: "created-1" } as never, createInput(), undefined, {} as never);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: adminKeys.workspaces });
    expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE);
  });
});

describe("useCreateInstanceWorkspace", () => {
  const createdWorkspace = {
    id: "33333333-3333-4333-8333-333333333333",
    name: "Good Name",
    slug: "good-slug-1",
    logo: null,
    logo_asset: null,
    logo_url: null,
    owner: {
      id: "22222222-2222-4222-8222-222222222222",
      email: "admin@example.com",
      first_name: "Admin",
      last_name: "User",
    },
    organization_size: "2-10",
    timezone: "UTC",
    background_color: "#ffffff",
    created_by: null,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };

  function CreateProbe(): React.ReactElement {
    const creating = useCreateInstanceWorkspace();
    if (creating.isSuccess) return <div data-testid="probe">created</div>;
    if (creating.isError) return <div data-testid="probe">failed</div>;
    return (
      <button
        type="button"
        data-testid="fire"
        onClick={() => creating.mutate({ name: "Good Name", slug: "good-slug-1", organization_size: "2-10" })}
      >
        fire
      </button>
    );
  }

  it("creates through the app client after a clear probe", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({
        "/api/instances/workspace-slug-check/": { status: 200, body: { status: true } },
        "/api/instances/workspaces/": { status: 201, body: createdWorkspace },
      })
    );
    renderProbe(CreateProbe);
    fireEvent.click(screen.getByTestId("fire"));
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("created"));
    expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_CREATE_SUCCESS_TITLE, WORKSPACE_CREATE_SUCCESS_MESSAGE);
  });
});
