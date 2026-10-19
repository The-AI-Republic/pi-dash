// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: workspace developer integrations (NEWFRONT-533) against live
// Django. The seed script plants a connected GitHub integration and one
// GitHub + one GitLab provider account directly, because the connect and
// register endpoints verify credentials against the real provider and a
// scratch stack has none. The suite proves the reads, the permission
// boundary, disconnect semantics, and the validation refusals; the success
// shapes of connect/register are pinned by unit stubs in contracts.test.ts.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import {
  disconnectGithubIntegration,
  disconnectGitProviderAccount,
  getGithubIntegration,
  isVerified,
  listGitProviderAccounts,
  registerGitProviderAccount,
} from "../contracts/integrations.js";
import { createWorkspace, deleteWorkspace } from "../contracts/workspace-management.js";
import { CONTRACT_DEV_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

function freshSlug(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
}

async function withWorkspace(client: ApiClient, prefix: string, body: (slug: string) => Promise<void>): Promise<void> {
  const slug = freshSlug(prefix);
  await createWorkspace(client, { name: `Contract ${slug}`, slug });
  try {
    await body(slug);
  } finally {
    await deleteWorkspace(client, slug).catch(() => 0);
  }
}

/** Run an unsafe call and return the ApiError it rejects with, or null. */
async function refusal(work: Promise<unknown>): Promise<ApiError | null> {
  const caught = await work.then(
    () => null,
    (error: unknown) => error
  );
  return caught instanceof ApiError ? caught : null;
}

describe.skipIf(!contractEnabled)("github integration contract", () => {
  it("reads the seeded connected provider with its verified login", async () => {
    const client = await sharedSignedIn();
    const status = await getGithubIntegration(client, CONTRACT_DEV_WORKSPACE);
    expect(status.connected).toBe(true);
    expect(status.github_user_login).toBe("contract-gh");
    expect(isVerified(status)).toBe(true);
    expect(typeof status.verified_at).toBe("string");
  });

  it("reads a fresh workspace as disconnected, then disconnects it idempotently", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-ght", async (slug) => {
      await expect(getGithubIntegration(client, slug)).resolves.toEqual({ connected: false });
      await expect(disconnectGithubIntegration(client, slug)).resolves.toEqual({ connected: false });
      await expect(getGithubIntegration(client, slug)).resolves.toEqual({ connected: false });
    });
  });

  it("refuses a blank token without writing", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-ghblank", async (slug) => {
      const error = await refusal(
        client.sendJson("POST", `/api/workspaces/${slug}/integrations/github/connect/`, {
          json: { token: "   " },
        })
      );
      expect(error?.status).toBe(400);
      await expect(getGithubIntegration(client, slug)).resolves.toEqual({ connected: false });
    });
  });

  it("keeps a disconnect for the admin's own workspace", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-ghdisc", async (slug) => {
      await expect(disconnectGithubIntegration(client, slug)).resolves.toEqual({ connected: false });
      const status = await getGithubIntegration(client, slug);
      expect(status.connected).toBe(false);
      expect(status.github_user_login).toBeUndefined();
    });
  });

  it("refuses the status read for a workspace the caller does not belong to", async () => {
    // The endpoint gates on workspace membership before resolving the
    // slug, so an unknown workspace answers the permission refusal (403),
    // not a missing-object 404.
    const client = await sharedSignedIn();
    const error = await getGithubIntegration(client, "ct-gh-missing").then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(403);
  });
});

describe.skipIf(!contractEnabled)("git provider account contract", () => {
  it("lists the seeded GitHub and GitLab accounts with their capabilities", async () => {
    const client = await sharedSignedIn();
    const { accounts } = await listGitProviderAccounts(client, CONTRACT_DEV_WORKSPACE);
    const github = accounts.find(
      (account) => account.provider === "github" && account.external_account_login === "contract-gh"
    );
    const gitlab = accounts.find(
      (account) => account.provider === "gitlab" && account.external_account_login === "contract-gitlab"
    );
    expect(github).toBeDefined();
    expect(github?.status).toBe("connected");
    expect(github?.auth_type).toBe("pat");
    expect(github?.capabilities["read_issues"]).toBe(true);
    expect(gitlab).toBeDefined();
    expect(gitlab?.host_url).toBe("https://gitlab.com");
    expect(gitlab?.display_name).toBe("Contract GitLab");
  });

  it("starts a fresh workspace with no accounts", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-gitempty", async (slug) => {
      await expect(listGitProviderAccounts(client, slug)).resolves.toEqual({ accounts: [] });
    });
  });

  it("disconnects an account, leaving it listed as revoked", async () => {
    // Idempotent: the seeded GitLab row starts connected, and the backend's
    // disconnect is repeatable, so the suite stays green on a second run
    // without re-seeding.
    const client = await sharedSignedIn();
    const seeded = (await listGitProviderAccounts(client, CONTRACT_DEV_WORKSPACE)).accounts;
    const gitlab = seeded.find((account) => account.provider === "gitlab");
    expect(gitlab).toBeDefined();
    const disconnected = await disconnectGitProviderAccount(client, CONTRACT_DEV_WORKSPACE, gitlab?.id ?? "");
    expect(disconnected).toEqual({ connected: false });
    const after = (await listGitProviderAccounts(client, CONTRACT_DEV_WORKSPACE)).accounts;
    expect(after.find((account) => account.id === gitlab?.id)?.status).toBe("revoked");
    expect(after.find((account) => account.id === gitlab?.id)?.last_check_error).toBe("Provider account disconnected");
  });

  it("refuses unknown providers and blank tokens", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-gitbad", async (slug) => {
      const provider = await refusal(
        client.sendJson("POST", `/api/workspaces/${slug}/integrations/git/accounts/`, {
          json: { provider: "bitbucket", token: "token" },
        })
      );
      expect(provider?.status).toBe(400);
      const blank = await refusal(
        client.sendJson("POST", `/api/workspaces/${slug}/integrations/git/accounts/`, {
          json: { provider: "gitlab", token: "" },
        })
      );
      expect(blank?.status).toBe(400);
      await expect(listGitProviderAccounts(client, slug)).resolves.toEqual({ accounts: [] });
    });
  });

  it("refuses a GitLab host outside the instance allowlist", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-githost", async (slug) => {
      const error = await refusal(
        client.sendJson("POST", `/api/workspaces/${slug}/integrations/git/accounts/`, {
          json: { provider: "gitlab", token: "token", host_url: "https://gitlab.example.invalid" },
        })
      );
      expect(error?.status).toBe(403);
      expect(error?.message).toContain("host is not allowed");
    });
  });

  it("404s disconnect for an unknown account", async () => {
    const client = await sharedSignedIn();
    const error = await refusal(
      disconnectGitProviderAccount(client, CONTRACT_DEV_WORKSPACE, "00000000-0000-0000-0000-000000000000")
    );
    expect(error?.status).toBe(404);
  });

  it("registers through the typed contract only after provider verification", async () => {
    const client = await sharedSignedIn();
    await withWorkspace(client, "ct-gitreg", async (slug) => {
      const error = await refusal(
        registerGitProviderAccount(client, slug, { provider: "gitlab", token: "invalid-probe" })
      );
      expect(error?.status).toBe(401);
      await expect(listGitProviderAccounts(client, slug)).resolves.toEqual({ accounts: [] });
    });
  });
});
