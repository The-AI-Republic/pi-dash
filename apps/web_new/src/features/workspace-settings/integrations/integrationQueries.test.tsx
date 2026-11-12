// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Developer-integration data (parity/specs/workspace-settings.md §15–§17,
// WSET-016–018): key shape and fetch policy, account selection and
// identity fallbacks, submit gating, and what each write sends.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contracts } = vi.hoisted(() => ({
  contracts: {
    connectGithubIntegration: vi.fn(),
    disconnectGithubIntegration: vi.fn(),
    disconnectGitProviderAccount: vi.fn(),
    getGithubIntegration: vi.fn(),
    listGitProviderAccounts: vi.fn(),
    registerGitProviderAccount: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

import type { GitProviderAccount } from "@pidash/api-client";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";
import {
  accountIdentity,
  canDisconnectGithub,
  canSubmitToken,
  gitProviderAccountsQueryOptions,
  githubIdentity,
  githubIntegrationQueryOptions,
  integrationsKeys,
  selectActiveGitlabAccount,
  useConnectGithub,
  useDisconnectGithub,
  useDisconnectGitlab,
  useRegisterGitlab,
} from "./integrationQueries.js";

const SLUG = "acme";
const ACCOUNT = "123e4567-e89b-12d3-a456-426614174000";

function account(overrides: Partial<GitProviderAccount> = {}): GitProviderAccount {
  return {
    id: ACCOUNT,
    provider: "gitlab",
    host_url: "https://gitlab.com",
    auth_type: "pat",
    external_account_id: "42",
    external_account_login: "octo",
    display_name: "Octo",
    capabilities: { read: true },
    status: "connected",
    verified_at: "2026-10-01T00:00:00Z",
    last_check_error: "",
    ...overrides,
  };
}

function setup<T>(hook: () => T) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const refetch = vi.spyOn(queryClient, "refetchQueries");
  const wrapper: React.ComponentType<{ children?: unknown }> = ({ children }) => (
    <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
  );
  return { ...renderHook(hook, { wrapper }), queryClient, refetch };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("integration reads", () => {
  it("scopes both keys under the workspace settings integrations branch", () => {
    expect(integrationsKeys.github(SLUG)).toEqual(["ws", SLUG, "workspace-settings", "integrations", "github"]);
    expect(integrationsKeys.accounts(SLUG)).toEqual(["ws", SLUG, "workspace-settings", "integrations", "accounts"]);
    expect(integrationsKeys.all(SLUG)).toEqual(["ws", SLUG, "workspace-settings", "integrations"]);
  });

  it("reads GitHub status on mount only and calls the contract with the client", async () => {
    const row = { connected: true, github_user_login: "octo" };
    contracts.getGithubIntegration.mockResolvedValue(row);
    const options = githubIntegrationQueryOptions(getAppClient(), SLUG);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(row);
    expect(contracts.getGithubIntegration).toHaveBeenCalledWith(getAppClient(), SLUG);
  });

  it("reads the provider-account list through the same opinionated policy", async () => {
    contracts.listGitProviderAccounts.mockResolvedValue({ accounts: [account()] });
    const options = gitProviderAccountsQueryOptions(getAppClient(), SLUG);
    expect(options.refetchOnWindowFocus).toBe(false);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ accounts: [account()] });
    expect(contracts.listGitProviderAccounts).toHaveBeenCalledWith(getAppClient(), SLUG);
  });
});

describe("account selection and identity", () => {
  it("uses the first non-revoked GitLab row and ignores other providers", () => {
    const revoked = account({ id: "a", status: "revoked" });
    const github = account({ id: "b", provider: "github" });
    const active = account({ id: "c", status: "degraded" });
    const later = account({ id: "d" });
    expect(selectActiveGitlabAccount([revoked, github, active, later])).toBe(active);
    expect(selectActiveGitlabAccount([revoked, github])).toBeNull();
    expect(selectActiveGitlabAccount([])).toBeNull();
  });

  it("falls back from login to display name to provider name", () => {
    expect(accountIdentity(account())).toBe("octo");
    expect(accountIdentity(account({ external_account_login: "  ", display_name: "Display" }))).toBe("Display");
    expect(accountIdentity(account({ external_account_login: "", display_name: " " }))).toBe("GitLab");
    expect(accountIdentity(account({ provider: "github", external_account_login: "", display_name: "" }))).toBe(
      "GitHub"
    );
  });

  it("reads GitHub's verified login with the provider fallback", () => {
    expect(githubIdentity({ connected: true, github_user_login: "octo" })).toBe("octo");
    expect(githubIdentity({ connected: true, github_user_login: "   " })).toBe("GitHub");
    expect(githubIdentity({ connected: true, github_user_login: null })).toBe("GitHub");
    expect(githubIdentity({ connected: false })).toBe("GitHub");
  });

  it("gates submit on a non-blank token while idle, and gates disconnect on a connected row", () => {
    expect(canSubmitToken("", false)).toBe(false);
    expect(canSubmitToken("   ", false)).toBe(false);
    expect(canSubmitToken(" token ", false)).toBe(true);
    expect(canSubmitToken("token", true)).toBe(false);
    expect(canDisconnectGithub({ connected: true }, false)).toBe(true);
    expect(canDisconnectGithub({ connected: false }, false)).toBe(false);
    expect(canDisconnectGithub(undefined, false)).toBe(false);
    expect(canDisconnectGithub({ connected: true }, true)).toBe(false);
  });
});

describe("integration writes", () => {
  it("sends the GitHub token trimmed and re-reads the status", async () => {
    contracts.connectGithubIntegration.mockResolvedValue({ connected: true });
    contracts.getGithubIntegration.mockResolvedValue({ connected: true });
    const { result, refetch } = setup(() => useConnectGithub(SLUG));
    await act(async () => {
      await result.current.mutateAsync("  github_pat_abc ");
    });
    expect(contracts.connectGithubIntegration).toHaveBeenCalledWith(expect.anything(), SLUG, {
      token: "github_pat_abc",
    });
    expect(refetch).toHaveBeenCalledWith({ queryKey: integrationsKeys.github(SLUG) });
  });

  it("disconnects GitHub and re-reads the status", async () => {
    contracts.disconnectGithubIntegration.mockResolvedValue({ connected: false });
    const { result, refetch } = setup(() => useDisconnectGithub(SLUG));
    await act(async () => {
      await result.current.mutateAsync();
    });
    expect(contracts.disconnectGithubIntegration).toHaveBeenCalledWith(expect.anything(), SLUG);
    expect(refetch).toHaveBeenCalledWith({ queryKey: integrationsKeys.github(SLUG) });
  });

  it("registers a GitLab account with a personal token, sending the host only when typed", async () => {
    contracts.registerGitProviderAccount.mockResolvedValue(account());
    const { result, refetch } = setup(() => useRegisterGitlab(SLUG));
    await act(async () => {
      await result.current.mutateAsync({ token: " glpat-abc ", hostUrl: "  " });
    });
    expect(contracts.registerGitProviderAccount).toHaveBeenLastCalledWith(expect.anything(), SLUG, {
      provider: "gitlab",
      token: "glpat-abc",
      auth_type: "pat",
    });
    await act(async () => {
      await result.current.mutateAsync({ token: "glpat-abc", hostUrl: " https://gitlab.example.com " });
    });
    expect(contracts.registerGitProviderAccount).toHaveBeenLastCalledWith(expect.anything(), SLUG, {
      provider: "gitlab",
      token: "glpat-abc",
      auth_type: "pat",
      host_url: "https://gitlab.example.com",
    });
    expect(refetch).toHaveBeenCalledWith({ queryKey: integrationsKeys.accounts(SLUG) });
  });

  it("disconnects one GitLab account and re-reads the list", async () => {
    contracts.disconnectGitProviderAccount.mockResolvedValue({ connected: false });
    const { result, refetch } = setup(() => useDisconnectGitlab(SLUG));
    await act(async () => {
      await result.current.mutateAsync(ACCOUNT);
    });
    expect(contracts.disconnectGitProviderAccount).toHaveBeenCalledWith(expect.anything(), SLUG, ACCOUNT);
    expect(refetch).toHaveBeenCalledWith({ queryKey: integrationsKeys.accounts(SLUG) });
  });

  it("rejects through the mutation so the page can show its failure", async () => {
    contracts.connectGithubIntegration.mockRejectedValue(new Error("401"));
    const { result } = setup(() => useConnectGithub(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync("bad")).rejects.toThrow("401");
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });
});
