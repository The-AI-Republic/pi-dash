// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth landing: skeleton while config loads, sign-up switch and method
// cards over stored state, single-key saves with progress then
// success/failure notices, whole-group lock in flight, and the
// last-method guard with sign-up exempt.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { showToast } from "@pidash/kit";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager, notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { adminKeys } from "../queries.js";
import { resetAdminStores } from "../stores.js";
import {
  AdminAuthLanding,
  AUTH_GUARD_MESSAGE,
  AUTH_GUARD_TITLE,
  AUTH_LANDING_SKELETON_BARS,
  AUTH_LANDING_TITLE,
  AUTH_SAVE_FAILED_MESSAGE,
  AUTH_SAVE_FAILED_TITLE,
  AUTH_SAVE_PROGRESS_TITLE,
  AUTH_SAVE_SUCCESS_MESSAGE,
  AUTH_SAVE_SUCCESS_TITLE,
} from "./AdminAuthLanding.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifySuccess: vi.fn(), notifyError: vi.fn() };
});

vi.mock("@pidash/kit", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  const realShowToast = original["showToast"] as (...args: never[]) => string;
  return { ...original, showToast: vi.fn((...args: never[]) => realShowToast(...args)) };
});

const mockedNotifySuccess = vi.mocked(notifySuccess);
const mockedNotifyError = vi.mocked(notifyError);
const mockedShowToast = vi.mocked(showToast);

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

const FULL_CONFIG: Record<string, string> = {
  ENABLE_SIGNUP: "1",
  ENABLE_MAGIC_LINK_LOGIN: "1",
  ENABLE_EMAIL_PASSWORD: "1",
  IS_GOOGLE_ENABLED: "0",
  IS_GITHUB_ENABLED: "1",
  IS_GITLAB_ENABLED: "0",
  IS_GITEA_ENABLED: "0",
  GOOGLE_CLIENT_ID: "",
  GOOGLE_CLIENT_SECRET: "",
  GITHUB_CLIENT_ID: "github-id",
  GITHUB_CLIENT_SECRET: "github-secret",
  GITLAB_CLIENT_ID: "",
  GITLAB_CLIENT_SECRET: "",
  GITEA_HOST: "",
  GITEA_CLIENT_ID: "",
  GITEA_CLIENT_SECRET: "",
};

function seedConfig(queryClient: QueryClient, values: Record<string, string>): void {
  queryClient.setQueryData(
    adminKeys.configurations,
    Object.entries(values).map(([key, value]) => configRow(key, value))
  );
}

interface PatchStub {
  bodies: unknown[];
  release: () => void;
}

function stubConfigTransport(args: {
  getBody?: unknown[];
  getStatus?: number;
  patchStatus?: number;
  hangGet?: boolean;
  hangPatch?: boolean;
}): PatchStub & { transport: Transport } {
  const bodies: unknown[] = [];
  let release = () => {};
  const gate = args.hangPatch === true ? new Promise<void>((resolve) => (release = resolve)) : null;
  const transport = (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    const path = new URL(url, "https://api.test").pathname;
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf" })),
      };
    }
    if (path === "/api/instances/configurations/" && method === "GET") {
      if (args.hangGet === true) await new Promise(() => {});
      return {
        status: args.getStatus ?? 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(args.getBody ?? [])),
      };
    }
    if (path === "/api/instances/configurations/" && method === "PATCH") {
      bodies.push(init?.body === undefined ? null : JSON.parse(init.body));
      if (gate !== null) await gate;
      const answered = Object.entries((bodies[bodies.length - 1] ?? {}) as Record<string, string>).map(([key, value]) =>
        configRow(key, value)
      );
      return {
        status: args.patchStatus ?? 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(args.patchStatus === 500 ? { detail: "boom" } : answered)),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: () => Promise.resolve("{}") };
  }) as Transport;
  return { bodies, release, transport };
}

function renderLanding(queryClient: QueryClient): void {
  render(
    <QueryClientProvider client={queryClient}>
      <AdminAuthLanding />
    </QueryClientProvider>
  );
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("AdminAuthLanding", () => {
  it("shows a five-bar skeleton while configuration loads", () => {
    createAppClient("https://api.test", stubConfigTransport({ hangGet: true }).transport);
    renderLanding(freshClient());
    expect(screen.getByTestId("admin-auth-landing")).toHaveTextContent("Authentication");
    const skeleton = screen.getByTestId("admin-auth-skeleton");
    expect(skeleton).toHaveAttribute("role", "status");
    expect(AUTH_LANDING_SKELETON_BARS).toBe(5);
    expect(skeleton.children).toHaveLength(5);
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("keeps the skeleton when the fetch fails", async () => {
    createAppClient("https://api.test", stubConfigTransport({ getStatus: 500, getBody: [] }).transport);
    renderLanding(freshClient());
    await waitFor(() => expect(screen.getByTestId("admin-auth-skeleton")).toBeInTheDocument());
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders the sign-up switch and the six cards in fixed order over stored state", async () => {
    createAppClient("https://api.test", stubConfigTransport({}).transport);
    const queryClient = freshClient();
    seedConfig(queryClient, FULL_CONFIG);
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-signup")).toBeInTheDocument());
    expect(document.title).toBe(AUTH_LANDING_TITLE);
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("noindex, nofollow");

    expect(screen.getByTestId("admin-auth-signup")).toHaveAttribute("aria-checked", "true");
    const ordered = within(screen.getByTestId("admin-auth-landing"))
      .getAllByTestId(/^admin-auth-card-/)
      .map((card) => card.getAttribute("data-testid"));
    expect(ordered).toEqual([
      "admin-auth-card-unique-codes",
      "admin-auth-card-passwords",
      "admin-auth-card-google",
      "admin-auth-card-github",
      "admin-auth-card-gitlab",
      "admin-auth-card-gitea",
    ]);
    expect(screen.getByTestId("admin-auth-card-passwords")).toHaveTextContent(
      "Allow members to create accounts with passwords and use it with their email addresses to sign in."
    );

    // Magic-link and password cards are toggle-only; GitHub is
    // configured (edit plus toggle); the rest offer configure.
    expect(screen.queryByTestId("admin-auth-link-unique-codes")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-toggle-passwords")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("admin-auth-link-github")).toHaveTextContent("Edit");
    expect(screen.getByTestId("admin-auth-toggle-github")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("admin-auth-link-google")).toHaveTextContent("Configure");
    expect(screen.queryByTestId("admin-auth-toggle-google")).not.toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-card-unique-codes").textContent ?? "").toContain("SMTP");
  });

  it("persists the sign-up flip as a single key with progress then success", async () => {
    const stub = stubConfigTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, FULL_CONFIG);
    const closer = vi.spyOn(getToastManager(), "close");
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-signup")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-signup"));
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith(AUTH_SAVE_SUCCESS_TITLE, AUTH_SAVE_SUCCESS_MESSAGE)
    );
    expect(stub.bodies).toEqual([{ ENABLE_SIGNUP: "0" }]);
    expect(mockedShowToast).toHaveBeenCalledWith(expect.anything(), { title: AUTH_SAVE_PROGRESS_TITLE, timeout: 0 });
    const progressId = mockedShowToast.mock.results[0]?.value as string;
    expect(closer).toHaveBeenCalledWith(progressId);
    expect(screen.getByTestId("admin-auth-signup")).toHaveAttribute("aria-checked", "false");
    expect(mockedNotifyError).not.toHaveBeenCalled();
  });

  it("persists a method flip the same way", async () => {
    const stub = stubConfigTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, { ...FULL_CONFIG, ENABLE_MAGIC_LINK_LOGIN: "0" });
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-toggle-unique-codes")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-toggle-unique-codes"));
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith(AUTH_SAVE_SUCCESS_TITLE, AUTH_SAVE_SUCCESS_MESSAGE)
    );
    expect(stub.bodies).toEqual([{ ENABLE_MAGIC_LINK_LOGIN: "1" }]);
    expect(screen.getByTestId("admin-auth-toggle-unique-codes")).toHaveAttribute("aria-checked", "true");
  });

  it("locks every switch behind the progress notice while a save flies", async () => {
    const stub = stubConfigTransport({ hangPatch: true });
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, FULL_CONFIG);
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-signup")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-signup"));
    await waitFor(() =>
      expect(mockedShowToast).toHaveBeenCalledWith(expect.anything(), { title: AUTH_SAVE_PROGRESS_TITLE, timeout: 0 })
    );
    for (const toggle of screen.getAllByRole("switch")) {
      expect(toggle).toHaveAttribute("aria-disabled", "true");
    }
    stub.release();
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith(AUTH_SAVE_SUCCESS_TITLE, AUTH_SAVE_SUCCESS_MESSAGE)
    );
    for (const toggle of screen.getAllByRole("switch")) {
      expect(toggle).not.toHaveAttribute("aria-disabled", "true");
    }
  });

  it("reports failure and keeps the stored value when the save fails", async () => {
    const stub = stubConfigTransport({ patchStatus: 500 });
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, FULL_CONFIG);
    const errored = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      renderLanding(queryClient);
      await waitFor(() => expect(screen.getByTestId("admin-auth-signup")).toBeInTheDocument());
      fireEvent.click(screen.getByTestId("admin-auth-signup"));
      await waitFor(() =>
        expect(mockedNotifyError).toHaveBeenCalledWith(AUTH_SAVE_FAILED_TITLE, AUTH_SAVE_FAILED_MESSAGE)
      );
      expect(stub.bodies).toEqual([{ ENABLE_SIGNUP: "0" }]);
      expect(screen.getByTestId("admin-auth-signup")).toHaveAttribute("aria-checked", "true");
      expect(screen.getByTestId("admin-auth-signup")).not.toHaveAttribute("aria-disabled", "true");
      expect(mockedNotifySuccess).not.toHaveBeenCalled();
    } finally {
      errored.mockRestore();
    }
  });

  it("refuses the lone enabled method with an error notice and no request", async () => {
    const stub = stubConfigTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, {
      ...FULL_CONFIG,
      ENABLE_SIGNUP: "1",
      ENABLE_MAGIC_LINK_LOGIN: "1",
      ENABLE_EMAIL_PASSWORD: "0",
      IS_GOOGLE_ENABLED: "0",
      IS_GITHUB_ENABLED: "0",
      IS_GITLAB_ENABLED: "0",
      IS_GITEA_ENABLED: "0",
    });
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-toggle-unique-codes")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-toggle-unique-codes"));
    await waitFor(() => expect(mockedNotifyError).toHaveBeenCalledWith(AUTH_GUARD_TITLE, AUTH_GUARD_MESSAGE));
    expect(stub.bodies).toEqual([]);
    expect(screen.getByTestId("admin-auth-toggle-unique-codes")).toHaveAttribute("aria-checked", "true");
  });

  it("lets sign-up flip freely while one method guards the minimum", async () => {
    const stub = stubConfigTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, {
      ...FULL_CONFIG,
      ENABLE_MAGIC_LINK_LOGIN: "1",
      ENABLE_EMAIL_PASSWORD: "0",
      IS_GOOGLE_ENABLED: "0",
      IS_GITHUB_ENABLED: "0",
      IS_GITLAB_ENABLED: "0",
      IS_GITEA_ENABLED: "0",
    });
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-signup")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-signup"));
    await waitFor(() =>
      expect(mockedNotifySuccess).toHaveBeenCalledWith(AUTH_SAVE_SUCCESS_TITLE, AUTH_SAVE_SUCCESS_MESSAGE)
    );
    expect(stub.bodies).toEqual([{ ENABLE_SIGNUP: "0" }]);
  });

  it("frees the first method once a second one switches on", async () => {
    const stub = stubConfigTransport({});
    createAppClient("https://api.test", stub.transport);
    const queryClient = freshClient();
    seedConfig(queryClient, {
      ...FULL_CONFIG,
      ENABLE_MAGIC_LINK_LOGIN: "1",
      ENABLE_EMAIL_PASSWORD: "0",
      IS_GOOGLE_ENABLED: "0",
      IS_GITHUB_ENABLED: "0",
      IS_GITLAB_ENABLED: "0",
      IS_GITEA_ENABLED: "0",
    });
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-toggle-passwords")).toBeInTheDocument());

    fireEvent.click(screen.getByTestId("admin-auth-toggle-passwords"));
    await waitFor(() => expect(stub.bodies).toEqual([{ ENABLE_EMAIL_PASSWORD: "1" }]));
    fireEvent.click(screen.getByTestId("admin-auth-toggle-unique-codes"));
    await waitFor(() =>
      expect(stub.bodies).toEqual([{ ENABLE_EMAIL_PASSWORD: "1" }, { ENABLE_MAGIC_LINK_LOGIN: "0" }])
    );
    expect(mockedNotifyError).not.toHaveBeenCalledWith(AUTH_GUARD_TITLE, AUTH_GUARD_MESSAGE);
  });

  it("trades configure for edit plus toggle as credentials land", async () => {
    createAppClient("https://api.test", stubConfigTransport({}).transport);
    const queryClient = freshClient();
    seedConfig(queryClient, {
      ...FULL_CONFIG,
      GITHUB_CLIENT_ID: "",
      GITHUB_CLIENT_SECRET: "",
      GITEA_CLIENT_ID: "gitea-id",
      GITEA_CLIENT_SECRET: "gitea-secret",
      GITEA_HOST: "",
    });
    renderLanding(queryClient);
    await waitFor(() => expect(screen.getByTestId("admin-auth-link-github")).toBeInTheDocument());
    expect(screen.getByTestId("admin-auth-link-github")).toHaveTextContent("Configure");
    // Credentials alone leave Gitea unconfigured: the host is required too.
    expect(screen.getByTestId("admin-auth-link-gitea")).toHaveTextContent("Configure");

    seedConfig(queryClient, {
      ...FULL_CONFIG,
      GITHUB_CLIENT_ID: "github-id",
      GITHUB_CLIENT_SECRET: "github-secret",
      GITEA_CLIENT_ID: "gitea-id",
      GITEA_CLIENT_SECRET: "gitea-secret",
      GITEA_HOST: "https://gitea.example.test",
    });
    await waitFor(() => expect(screen.getByTestId("admin-auth-link-github")).toHaveTextContent("Edit"));
    expect(screen.getByTestId("admin-auth-toggle-github")).toBeInTheDocument();
    expect(screen.getByTestId("admin-auth-link-gitea")).toHaveTextContent("Edit");
    expect(screen.getByTestId("admin-auth-link-gitea")).toHaveAttribute("href", "/god-mode/authentication/gitea/");
  });
});
