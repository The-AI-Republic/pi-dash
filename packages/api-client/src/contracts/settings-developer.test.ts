// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stub-transport tests for the developer settings contracts (NEWFRONT-533):
// each function's URL, method and body, plus the shapes the live suites
// cannot create (a connected GitHub answer, a registered Git account, the
// one-time secrets). Billing plan data is pure and pinned here.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  connectGithubIntegration,
  disconnectGithubIntegration,
  disconnectGitProviderAccount,
  getGithubIntegration,
  isVerified,
  listGitProviderAccounts,
  registerGitProviderAccount,
} from "./integrations.js";
import {
  allWebhookEvents,
  createWorkspaceWebhook,
  deleteWorkspaceWebhook,
  getWorkspaceWebhook,
  listWorkspaceWebhooks,
  regenerateWorkspaceWebhookSecret,
  updateWorkspaceWebhook,
  webhookUsesAllEvents,
} from "./webhooks.js";
import { createPersonalApiToken, deletePersonalApiToken, listPersonalApiTokens } from "./api-tokens.js";
import { billingPlanAction, billingPlanDiscount, billingPlanIsPopular, billingPlanPrice } from "./billing-plans.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";
const AT = "2026-10-16T15:32:05.479661+09:00";

interface Call {
  method: string;
  url: string;
  json?: unknown;
}

function recorder(
  response: unknown,
  options: { status?: number; reject?: unknown; rawUrl?: string } = {}
): { client: ApiClient; calls: Call[] } {
  const calls: Call[] = [];
  const settle = async (): Promise<unknown> => {
    if (options.reject !== undefined) throw options.reject;
    return response;
  };
  const client = {
    // Mirror ApiClient.parse: a schema violation becomes a contract ApiError,
    // which is what callers and tests observe against the real client.
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) => {
      try {
        return schema.parse(value);
      } catch (error) {
        throw new ApiError({ status: 0, code: "contract", message: String(error) });
      }
    }) as ApiClient["parse"],
    getJson: (async (url: string) => {
      calls.push({ method: "GET", url });
      return settle();
    }) as ApiClient["getJson"],
    sendJson: (async (method: string, url: string, init: { json?: unknown } = {}) => {
      calls.push({ method, url, json: init.json });
      return { data: await settle(), status: options.status ?? 200, url: `${url}${options.rawUrl ?? ""}` };
    }) as ApiClient["sendJson"],
    delete: (async (url: string) => {
      calls.push({ method: "DELETE", url });
      if (options.reject !== undefined) throw options.reject;
      return {
        status: options.status ?? 204,
        url: `${url}${options.rawUrl ?? ""}`,
        headers: { get: () => null },
        text: async () => "",
      };
    }) as ApiClient["delete"],
  } as unknown as ApiClient;
  return { client, calls };
}

const GITHUB_STATUS = {
  connected: true,
  github_user_login: "octo",
  verified_at: "2026-01-02T03:04:05+00:00",
};

const ACCOUNT = {
  id: UUID,
  provider: "gitlab",
  host_url: "https://gitlab.com",
  auth_type: "pat",
  external_account_id: "42",
  external_account_login: "octo",
  display_name: "Octo",
  capabilities: { read_issues: true, clone: false },
  status: "connected",
  verified_at: AT,
  last_check_error: "",
  credential_config: { token: "must-not-leak" },
};

const WEBHOOK = {
  id: UUID,
  url: "https://example.com/hook",
  is_active: true,
  created_at: AT,
  updated_at: AT,
  project: true,
  issue: true,
  cycle: false,
  module: true,
  issue_comment: false,
};

const TOKEN_ROW = {
  id: UUID2,
  is_active: true,
  created_at: AT,
  updated_at: AT,
  label: "ci",
  description: "deploy",
  last_used: null,
  user_type: 0,
  expired_at: null,
  is_service: false,
  allowed_rate_limit: "60/min",
  created_by: null,
  updated_by: null,
  user: UUID,
  workspace: null,
};

describe("github integration contract", () => {
  it("reads the status from the workspace provider path", async () => {
    const { client, calls } = recorder(GITHUB_STATUS);
    const status = await getGithubIntegration(client, "acme");
    expect(calls).toEqual([{ method: "GET", url: "/api/workspaces/acme/integrations/github/" }]);
    expect(status).toEqual({ connected: true, github_user_login: "octo", verified_at: "2026-01-02T03:04:05+00:00" });
    expect(isVerified(status)).toBe(true);
  });

  it("reads the disconnected shape and treats it as unverified", async () => {
    const { client } = recorder({ connected: false });
    const status = await getGithubIntegration(client, "acme");
    expect(status).toEqual({ connected: false });
    expect(isVerified(status)).toBe(false);
  });

  it("connects with the token in the canonical body", async () => {
    const { client, calls } = recorder(GITHUB_STATUS);
    const status = await connectGithubIntegration(client, "acme", { token: "ghp_x" });
    expect(calls).toEqual([
      { method: "POST", url: "/api/workspaces/acme/integrations/github/connect/", json: { token: "ghp_x" } },
    ]);
    expect(status.connected).toBe(true);
  });

  it("disconnects with an empty body", async () => {
    const { client, calls } = recorder({ connected: false });
    await disconnectGithubIntegration(client, "acme");
    expect(calls).toEqual([{ method: "POST", url: "/api/workspaces/acme/integrations/github/disconnect/", json: {} }]);
  });

  it("rejects a status without the connected flag", async () => {
    const { client } = recorder({ github_user_login: "octo" });
    const error = await getGithubIntegration(client, "acme").catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });
});

describe("git provider account contract", () => {
  it("lists accounts and drops credential material from each row", async () => {
    const { client, calls } = recorder({ accounts: [ACCOUNT] });
    const list = await listGitProviderAccounts(client, "acme");
    expect(calls).toEqual([{ method: "GET", url: "/api/workspaces/acme/integrations/git/accounts/" }]);
    expect(list.accounts).toHaveLength(1);
    expect(list.accounts[0]).not.toHaveProperty("credential_config");
    expect(list.accounts[0]?.capabilities["read_issues"]).toBe(true);
  });

  it("registers with the provider, token, auth type and host", async () => {
    const { client, calls } = recorder(ACCOUNT);
    const account = await registerGitProviderAccount(client, "acme", {
      provider: "gitlab",
      token: "glpat_x",
      host_url: "https://gitlab.com",
      auth_type: "pat",
    });
    expect(calls[0]).toEqual({
      method: "POST",
      url: "/api/workspaces/acme/integrations/git/accounts/",
      json: { provider: "gitlab", token: "glpat_x", host_url: "https://gitlab.com", auth_type: "pat" },
    });
    expect(account.id).toBe(UUID);
  });

  it("disconnects through the account detail path", async () => {
    const { client, calls } = recorder({ connected: false });
    const result = await disconnectGitProviderAccount(client, "acme", UUID);
    expect(calls).toEqual([{ method: "DELETE", url: `/api/workspaces/acme/integrations/git/accounts/${UUID}/` }]);
    expect(result).toEqual({ connected: false });
  });

  it("rejects an unknown provider and status", async () => {
    const badProvider = recorder({ ...ACCOUNT, provider: "bitbucket" });
    await expect(listGitProviderAccounts(badProvider.client, "acme")).rejects.toBeInstanceOf(ApiError);
    const badStatus = recorder({ ...ACCOUNT, status: "paused" });
    await expect(listGitProviderAccounts(badStatus.client, "acme")).rejects.toBeInstanceOf(ApiError);
  });
});

describe("workspace webhook contract", () => {
  it("lists, reads, creates, patches and deletes on the canonical paths", async () => {
    const list = recorder([WEBHOOK]);
    expect(await listWorkspaceWebhooks(list.client, "acme")).toHaveLength(1);
    expect(list.calls[0]?.url).toBe("/api/workspaces/acme/webhooks/");

    const detail = recorder(WEBHOOK);
    await getWorkspaceWebhook(detail.client, "acme", UUID);
    expect(detail.calls[0]?.url).toBe(`/api/workspaces/acme/webhooks/${UUID}/`);

    const created = recorder({ ...WEBHOOK, secret_key: "pi_dash_wh_x" }, { status: 201 });
    const input = { url: "https://example.com/hook", ...allWebhookEvents() };
    const result = await createWorkspaceWebhook(created.client, "acme", input);
    expect(created.calls[0]).toEqual({ method: "POST", url: "/api/workspaces/acme/webhooks/", json: input });
    expect(result.secret_key).toBe("pi_dash_wh_x");

    const patched = recorder({ ...WEBHOOK, is_active: false });
    await updateWorkspaceWebhook(patched.client, "acme", UUID, { is_active: false });
    expect(patched.calls[0]).toEqual({
      method: "PATCH",
      url: `/api/workspaces/acme/webhooks/${UUID}/`,
      json: { is_active: false },
    });

    const removed = recorder(undefined);
    expect(await deleteWorkspaceWebhook(removed.client, "acme", UUID)).toBe(204);
    expect(removed.calls[0]?.url).toBe(`/api/workspaces/acme/webhooks/${UUID}/`);
  });

  it("regenerates on the regenerate path with an empty body", async () => {
    const { client, calls } = recorder({ ...WEBHOOK, secret_key: "pi_dash_wh_new" });
    const result = await regenerateWorkspaceWebhookSecret(client, "acme", UUID);
    expect(calls[0]).toEqual({
      method: "POST",
      url: `/api/workspaces/acme/webhooks/${UUID}/regenerate/`,
      json: {},
    });
    expect(result.secret_key).toBe("pi_dash_wh_new");
  });

  it("keeps the create answer secret required", async () => {
    const { client } = recorder(WEBHOOK, { status: 201 });
    const error = await createWorkspaceWebhook(client, "acme", {
      url: "https://example.com",
      ...allWebhookEvents(),
    }).catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("contract");
  });

  it("never exposes the secret echoed by a webhook read", async () => {
    // The backend's read serializer still sends secret_key although the spec
    // says reads must not. The read row declares no such field, so the
    // contract's strict parse drops it before any slice can see it; the
    // create/regenerate answers below still carry the one-time key.
    const { client } = recorder({ ...WEBHOOK, secret_key: "pi_dash_wh_leak" });
    const row = await getWorkspaceWebhook(client, "acme", UUID);
    expect(row.id).toBe(UUID);
    expect(row).not.toHaveProperty("secret_key");
    const list = await listWorkspaceWebhooks(recorder([{ ...WEBHOOK, secret_key: "pi_dash_wh_leak" }]).client, "acme");
    expect(list[0]).not.toHaveProperty("secret_key");
  });

  it("computes all-events mode from the five flags", () => {
    const base = { ...WEBHOOK };
    expect(
      webhookUsesAllEvents({ ...base, project: true, issue: true, cycle: true, module: true, issue_comment: true })
    ).toBe(true);
    expect(webhookUsesAllEvents(base)).toBe(false);
    expect(allWebhookEvents()).toEqual({ project: true, issue: true, cycle: true, module: true, issue_comment: true });
  });
});

describe("personal API token contract", () => {
  it("lists the caller's tokens without the secret", async () => {
    const { client, calls } = recorder([TOKEN_ROW]);
    const rows = await listPersonalApiTokens(client);
    expect(calls).toEqual([{ method: "GET", url: "/api/users/api-tokens/" }]);
    expect(rows[0]?.label).toBe("ci");
    expect(rows[0]).not.toHaveProperty("token");
  });

  it("creates with the label, description and expiry and reads the one-time secret", async () => {
    const { client, calls } = recorder({ ...TOKEN_ROW, token: "pi_dash_api_x" }, { status: 201 });
    const created = await createPersonalApiToken(client, { label: "ci", description: "deploy", expired_at: null });
    expect(calls).toEqual([
      {
        method: "POST",
        url: "/api/users/api-tokens/",
        json: { label: "ci", description: "deploy", expired_at: null },
      },
    ]);
    expect(created.token).toBe("pi_dash_api_x");
  });

  it("creates with an ISO expiry when one is chosen", async () => {
    const expiry = "2027-01-01T12:00:00.000Z";
    const { client, calls } = recorder({ ...TOKEN_ROW, expired_at: expiry, token: "pi_dash_api_y" }, { status: 201 });
    await createPersonalApiToken(client, { label: "ci", description: "", expired_at: expiry });
    expect(calls[0]?.json).toEqual({ label: "ci", description: "", expired_at: expiry });
  });

  it("deletes the slashless item address the spec names", async () => {
    const { client, calls } = recorder(undefined);
    expect(await deletePersonalApiToken(client, UUID2)).toBe(204);
    expect(calls).toEqual([{ method: "DELETE", url: `/api/users/api-tokens/${UUID2}` }]);
  });

  it("rejects a row with a wrong liveness type", async () => {
    const { client } = recorder([{ ...TOKEN_ROW, is_active: "yes" }]);
    await expect(listPersonalApiTokens(client)).rejects.toBeInstanceOf(ApiError);
  });
});

describe("billing plan data", () => {
  it("opens the per-frequency Pro pricing page", () => {
    expect(billingPlanAction("Pro", "month")).toEqual({
      label: "Upgrade to Pro",
      target: "https://airepublic.com/pricing/pro?plan=month",
    });
    expect(billingPlanAction("Pro", "year").target).toBe("https://airepublic.com/pricing/pro?plan=year");
  });

  it("keeps the Business sales label while landing on its pricing page", () => {
    expect(billingPlanAction("Business", "month")).toEqual({
      label: "Talk to Sales",
      target: "https://airepublic.com/pricing/business?plan=month",
    });
    expect(billingPlanAction("Business", "year").target).toBe("https://airepublic.com/pricing/business?plan=year");
  });

  it("sends Enterprise to the sales contact page at either frequency", () => {
    expect(billingPlanAction("Enterprise", "month")).toEqual({
      label: "Talk to Sales",
      target: "https://airepublic.com/contact",
    });
    expect(billingPlanAction("Enterprise", "year").target).toBe("https://airepublic.com/contact");
  });

  it("prices Pro per frequency and quotes the others", () => {
    expect(billingPlanPrice("Pro", "month")).toBe("$8");
    expect(billingPlanPrice("Pro", "year")).toBe("$6");
    expect(billingPlanPrice("Business", "month")).toBe("Quote on request");
    expect(billingPlanPrice("Enterprise", "year")).toBe("Quote on request");
  });

  it("marks only Pro popular and badges only its yearly price", () => {
    expect(billingPlanIsPopular("Pro")).toBe(true);
    expect(billingPlanIsPopular("Business")).toBe(false);
    expect(billingPlanDiscount("Pro", "year")).toBe("-25%");
    expect(billingPlanDiscount("Pro", "month")).toBeNull();
    expect(billingPlanDiscount("Business", "year")).toBeNull();
  });
});
