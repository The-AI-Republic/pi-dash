// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-412): the workspace-settings developer group —
// the integrations page with its GitHub and GitLab provider cards, and the
// webhooks list, create dialog, secret reveal, detail page and delete flow.
// All pages are admin-only; members get the shared not-authorized view
// everywhere (the settings layout intercepts before the detail page's own
// paragraph can render).
//
// Seed coupling (deliberate, documented): provider connects verify their
// tokens against the live providers, so no UI connect can succeed from the
// seeded stack. The invalid-token path runs through the UI — and trips an
// old bug (NEWFRONT-421): the provider's 401 hits the global session
// interceptor, which reloads the page and swallows the failure notice, so
// those two scenarios pin the reload as bug:. The connected and disconnect
// states plant server-side through the Django shell — mirroring what the
// connect endpoints store — and the UI disconnect plus its sync cascade are
// proven from there. Webhook secrets are asserted end to end: masked
// reveal, show/hide, clipboard copy, the auto-downloaded CSV backup, and
// one-time-only clearing on close (the regenerate backup is itself buggy —
// NEWFRONT-419 — and pinned separately as bug:).
// Rows: WSET-016..024, WSET-041.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCleanupWebhook,
  serverCreateProjectWithFlags,
  serverCreateWebhook,
  serverGitAccounts,
  serverGithubStatus,
  serverPlantGitSyncBinding,
  serverRepositoryBinding,
  serverSeedGithubConnection,
  serverUnseedGitAccount,
  serverUnseedGithubConnection,
  serverWebhookDetails,
  serverWebhooksList,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

function requireMember(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.mentionMember) throw new Error("[parity] seed has no mention member; rerun parity-up.sh.");
  return { email: seed.mentionMember.email, password: seed.mentionMember.password };
}

/** Unique, resolvable target URL per test (workspace+URL is unique). */
function webhookUrl(label: string): string {
  return `https://example.com/post/412-${label}-${Date.now()}`;
}

/**
 * A failed write's notice. The old app titles every failure "Error!";
 * web_new names the outcome ("Webhook not updated"), so each leg pins
 * what its own surface says.
 */
function failureNotice(driver: ParityDriver, webNewNotice: string): string {
  return driver.target === "web_new" ? webNewNotice : "Error";
}

test(
  specTitle(["WSET-016"], "integrations page shows the banner heading, both provider cards and a titled tab"),
  { tag: specTags(["WSET-016"]) },
  async ({ driver, seed }) => {
    await serverUnseedGithubConnection(seed.workspaceSlug);
    const session = await signInSession(seed.email, seed.password);
    expect((await serverGithubStatus(seed.workspaceSlug, session)).connected).toBe(false);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.wsSettingsOpenIntegrations(seed.workspaceSlug);
    expect(await driver.wsSettingsIntegrationsNotAuthorized()).toBe(false);
    expect(await driver.wsSettingsIntegrationsHeadingVisible()).toBe(true);
    expect(await driver.wsSettingsIntegrationsProviderTitles()).toEqual(["GitHub", "GitLab"]);
    expect(await driver.wsSettingsPageTitle()).toBe(`${seed.workspaceName} - Integrations`);
    expect(await driver.wsSettingsGithubConnected()).toBe(false);
    expect(await driver.wsSettingsGitlabConnected()).toBe(false);
    expect(await driver.wsSettingsGithubDisconnectVisible()).toBe(false);
    expect(await driver.wsSettingsGitlabDisconnectVisible()).toBe(false);
    expect(await driver.wsSettingsGithubStatusText()).toContain("fine-grained PAT");
    expect(await driver.wsSettingsGitlabStatusText()).toContain("GitLab token");
  }
);

test(
  specTitle(["WSET-016"], "members get the not-authorized view on the integrations page"),
  { tag: specTags(["WSET-016"]) },
  async ({ driver, seed }) => {
    const member = requireMember(seed);
    await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
    await driver.wsSettingsOpenIntegrations(seed.workspaceSlug);
    expect(await driver.wsSettingsIntegrationsNotAuthorized()).toBe(true);
    expect(await driver.wsSettingsIntegrationsProviderTitles()).toEqual([]);
  }
);

test(
  specTitle(["WSET-017"], "bug: a rejected GitHub token reloads the page instead of a failure notice (NEWFRONT-421)"),
  { tag: specTags(["WSET-017"]) },
  async ({ driver, seed }) => {
    // Intended behavior (NEWFRONT-421): a rejected token shows the
    // failure notice inline, keeps the typed token and stays on the
    // page. The old app instead answers 401, which trips the global
    // session interceptor into a full reload that swallows the notice
    // and clears the card.
    const session = await signInSession(seed.email, seed.password);
    await serverUnseedGithubConnection(seed.workspaceSlug);
    expect((await serverGithubStatus(seed.workspaceSlug, session)).connected).toBe(false);
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.wsSettingsOpenIntegrations(seed.workspaceSlug);
    expect(await driver.wsSettingsGithubConnected()).toBe(false);
    expect(await driver.wsSettingsGithubConnectEnabled()).toBe(false);

    const bogus = "github_pat_invalid-parity-412-probe";
    await driver.wsSettingsGithubTokenFill(bogus);
    expect(await driver.wsSettingsGithubConnectEnabled()).toBe(true);
    await driver.wsSettingsGithubConnect();
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-421): the rejection stays inline, the typed
      // token survives and the card stays disconnected.
      await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Connection failed");
      expect(await driver.wsSettingsGithubTokenValue()).toBe(bogus);
      expect(await driver.wsSettingsGithubConnected()).toBe(false);
      expect(await driver.wsSettingsGithubDisconnectVisible()).toBe(false);
      expect((await serverGithubStatus(seed.workspaceSlug, session)).connected).toBe(false);
      return;
    }
    // Settle on the reloaded card: status resolved past Loading with the
    // token field back and empty.
    await expect
      .poll(
        async () => {
          const value = await driver.wsSettingsGithubTokenValue();
          const status = await driver.wsSettingsGithubStatusText();
          return status !== null && !status.includes("Loading") && value === "";
        },
        { timeout: 60_000 }
      )
      .toBe(true);
    expect(await driver.lastToast()).toBeNull();
    expect(await driver.wsSettingsGithubConnected()).toBe(false);
    expect(await driver.wsSettingsGithubDisconnectVisible()).toBe(false);
    expect(await driver.wsSettingsGithubConnectEnabled()).toBe(false);
    expect((await serverGithubStatus(seed.workspaceSlug, session)).connected).toBe(false);
  }
);

test(
  specTitle(["WSET-017"], "GitHub disconnect from the connected state disables dependent syncs"),
  { tag: specTags(["WSET-017"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await serverUnseedGithubConnection(seed.workspaceSlug);
    const login = "parity-gh-412";
    await serverSeedGithubConnection(seed.workspaceSlug, login);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `NF412 gh cascade ${Date.now()}`,
      parityProjectIdentifier("N412"),
      {},
      session
    );
    const cascade = await serverPlantGitSyncBinding(seed.workspaceSlug, projectId, {
      provider: "github",
      login: "parity-gh-sync",
      hostUrl: "https://github.com",
      linkWorkspaceIntegration: true,
    });
    // The seed project's own binding is untouched by this disconnect.
    const seedSyncBefore = (await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session)).syncEnabled;
    try {
      expect((await serverGithubStatus(seed.workspaceSlug, session)).github_user_login).toBe(login);
      expect((await serverRepositoryBinding(seed.workspaceSlug, projectId, session)).syncEnabled).toBe(true);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenIntegrations(seed.workspaceSlug);

      await expect
        .poll(() => driver.wsSettingsGithubStatusText(), { timeout: 30_000 })
        .toContain(`Connected as ${login}`);
      expect(await driver.wsSettingsGithubConnected()).toBe(true);
      expect(await driver.wsSettingsGithubDisconnectVisible()).toBe(true);
      await driver.wsSettingsGithubDisconnect();
      await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Disconnected");
      await expect.poll(() => driver.wsSettingsGithubConnected(), { timeout: 30_000 }).toBe(false);
      expect(await driver.wsSettingsGithubDisconnectVisible()).toBe(false);
      expect(await driver.wsSettingsGithubConnectEnabled()).toBe(false);

      await expect
        .poll(async () => (await serverGithubStatus(seed.workspaceSlug, session)).connected, { timeout: 20_000 })
        .toBe(false);
      const binding = await serverRepositoryBinding(seed.workspaceSlug, projectId, session);
      expect(binding.bound).toBe(true);
      expect(binding.syncEnabled).toBe(false);
      const accounts = await serverGitAccounts(seed.workspaceSlug, session);
      expect(accounts.find((row) => row.id === cascade.accountId)?.status).toBe("revoked");
      expect((await serverRepositoryBinding(seed.workspaceSlug, seed.projectId, session)).syncEnabled).toBe(
        seedSyncBefore
      );
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverUnseedGitAccount(cascade.accountId);
      await serverUnseedGithubConnection(seed.workspaceSlug);
    }
  }
);

test(
  specTitle(["WSET-018"], "bug: a rejected GitLab token reloads the page instead of a failure notice (NEWFRONT-421)"),
  { tag: specTags(["WSET-018"]) },
  async ({ driver, seed }) => {
    // Same interceptor bug as the GitHub card (NEWFRONT-421): the
    // provider's 401 reloads the page and swallows the failure notice.
    const session = await signInSession(seed.email, seed.password);
    // Convergent: clear any leaked GitLab plant (the seed's GitHub account
    // is never touched).
    for (const row of await serverGitAccounts(seed.workspaceSlug, session)) {
      if (row.provider === "gitlab" && row.status !== "revoked") await serverUnseedGitAccount(row.id);
    }
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
    await driver.wsSettingsOpenIntegrations(seed.workspaceSlug);
    expect(await driver.wsSettingsGitlabConnected()).toBe(false);
    expect(await driver.wsSettingsGitlabConnectEnabled()).toBe(false);

    const bogus = "glpat-invalid-parity-412-probe";
    await driver.wsSettingsGitlabTokenFill(bogus);
    expect(await driver.wsSettingsGitlabConnectEnabled()).toBe(true);
    await driver.wsSettingsGitlabConnect();
    if (driver.target === "web_new") {
      // Intended (NEWFRONT-421): inline failure, typed token kept and no
      // account registered.
      await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Connection failed");
      expect(await driver.wsSettingsGitlabTokenValue()).toBe(bogus);
      expect(await driver.wsSettingsGitlabConnected()).toBe(false);
      expect(await driver.wsSettingsGitlabDisconnectVisible()).toBe(false);
      const registered = await serverGitAccounts(seed.workspaceSlug, session);
      expect(registered.filter((row) => row.provider === "gitlab" && row.status !== "revoked")).toHaveLength(0);
      return;
    }
    // Settle on the reloaded card: status resolved past Loading with the
    // token field back and empty.
    await expect
      .poll(
        async () => {
          const value = await driver.wsSettingsGitlabTokenValue();
          const status = await driver.wsSettingsGitlabStatusText();
          return status !== null && !status.includes("Loading") && value === "";
        },
        { timeout: 60_000 }
      )
      .toBe(true);
    expect(await driver.lastToast()).toBeNull();
    expect(await driver.wsSettingsGitlabConnected()).toBe(false);
    expect(await driver.wsSettingsGitlabDisconnectVisible()).toBe(false);
    expect(await driver.wsSettingsGitlabConnectEnabled()).toBe(false);
    const accounts = await serverGitAccounts(seed.workspaceSlug, session);
    expect(accounts.filter((row) => row.provider === "gitlab" && row.status !== "revoked")).toHaveLength(0);
  }
);

test(
  specTitle(["WSET-018"], "GitLab disconnect from the connected state stops bound syncs"),
  { tag: specTags(["WSET-018"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const login = "parity-gl-412";
    const hostUrl = "https://gitlab.example.com";
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `NF412 gl cascade ${Date.now()}`,
      parityProjectIdentifier("N412"),
      {},
      session
    );
    // One planted account serves both the card and the cascade binding.
    const cascade = await serverPlantGitSyncBinding(seed.workspaceSlug, projectId, {
      provider: "gitlab",
      login,
      hostUrl,
      linkWorkspaceIntegration: false,
    });
    try {
      expect((await serverRepositoryBinding(seed.workspaceSlug, projectId, session)).syncEnabled).toBe(true);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenIntegrations(seed.workspaceSlug);

      await expect
        .poll(() => driver.wsSettingsGitlabStatusText(), { timeout: 30_000 })
        .toContain(`Connected as ${login} on ${hostUrl}`);
      expect(await driver.wsSettingsGitlabConnected()).toBe(true);
      expect(await driver.wsSettingsGitlabDisconnectVisible()).toBe(true);
      await driver.wsSettingsGitlabDisconnect();
      await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Disconnected");
      await expect.poll(() => driver.wsSettingsGitlabConnected(), { timeout: 30_000 }).toBe(false);
      expect(await driver.wsSettingsGitlabDisconnectVisible()).toBe(false);

      const accounts = await serverGitAccounts(seed.workspaceSlug, session);
      expect(accounts.find((row) => row.id === cascade.accountId)?.status).toBe("revoked");
      const binding = await serverRepositoryBinding(seed.workspaceSlug, projectId, session);
      expect(binding.bound).toBe(true);
      expect(binding.syncEnabled).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await serverUnseedGitAccount(cascade.accountId);
    }
  }
);

test(
  specTitle(["WSET-019"], "webhooks list runs the loader, the empty state and rows linking to detail"),
  { tag: specTags(["WSET-019"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    for (const row of await serverWebhooksList(seed.workspaceSlug, session)) {
      await serverCleanupWebhook(seed.workspaceSlug, row.id, session);
    }
    await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

    await test.step("a loader covers the list while the fetch is held", async () => {
      await driver.wsSettingsWebhooksDelayListFetch(3_000);
      const opened = driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
      await expect.poll(() => driver.wsSettingsWebhooksLoaderVisible(), { timeout: 30_000 }).toBe(true);
      await opened;
    });

    await test.step("the heading, Add entry and empty state render", async () => {
      expect(await driver.wsSettingsWebhooksHeadingVisible()).toBe(true);
      expect(await driver.wsSettingsWebhooksAddVisible()).toBe(true);
      expect(await driver.wsSettingsWebhooksEmptyVisible()).toBe(true);
    });

    await test.step("the empty-state action opens the same dialog", async () => {
      await driver.wsSettingsWebhooksEmptyAddClick();
      expect(await driver.wsSettingsWebhookDialogTitle()).toBe("Create webhook");
      await driver.wsSettingsWebhookCancel();
      await expect.poll(() => driver.wsSettingsWebhookDialogOpen(), { timeout: 20_000 }).toBe(false);
    });

    const url = webhookUrl("list");
    const created = await serverCreateWebhook(seed.workspaceSlug, { url }, session);
    try {
      await driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
      await expect.poll(() => driver.wsSettingsWebhooksRowUrls(), { timeout: 30_000 }).toContain(url);
      expect(await driver.wsSettingsWebhooksEmptyVisible()).toBe(false);
      await driver.wsSettingsWebhooksOpenRow(url);
      expect(await driver.wsSettingsWebhookUrlValue()).toBe(url);
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);

test(
  specTitle(["WSET-019"], "members get the not-authorized view on the webhooks list"),
  { tag: specTags(["WSET-019"]) },
  async ({ driver, seed }) => {
    const member = requireMember(seed);
    await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
    await driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
    expect(await driver.wsSettingsWebhooksNotAuthorized()).toBe(true);
    expect(await driver.wsSettingsWebhooksAddVisible()).toBe(false);
    expect(await driver.wsSettingsWebhooksEmptyVisible()).toBe(false);
  }
);

test(
  specTitle(["WSET-020"], "create webhook validates, picks events, cancels and lands on the reveal"),
  { tag: specTags(["WSET-020"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    for (const row of await serverWebhooksList(seed.workspaceSlug, session)) {
      await serverCleanupWebhook(seed.workspaceSlug, row.id, session);
    }
    const url = webhookUrl("create");
    let createdId = "";
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
      await driver.wsSettingsWebhooksAddClick();

      await test.step("the dialog defaults to all events with no toggles", async () => {
        expect(await driver.wsSettingsWebhookDialogTitle()).toBe("Create webhook");
        expect(await driver.wsSettingsWebhookEventMode()).toBe("all");
        expect(await driver.wsSettingsWebhookIndividualOptions()).toEqual([]);
      });

      await test.step("an empty URL blocks submit inline", async () => {
        await driver.wsSettingsWebhookSubmit();
        expect(await driver.wsSettingsWebhookUrlError()).toBe("URL is required");
        expect(await driver.wsSettingsWebhookDialogTitle()).toBe("Create webhook");
      });

      await test.step("individual mode offers five all-on toggles", async () => {
        await driver.wsSettingsWebhookPickEventMode("individual");
        expect(await driver.wsSettingsWebhookEventMode()).toBe("individual");
        const options = await driver.wsSettingsWebhookIndividualOptions();
        expect(options.map((row) => row.label)).toEqual([
          "Projects",
          "Cycles",
          "Work items",
          "Modules",
          "Work item comments",
        ]);
        expect(options.every((row) => row.checked)).toBe(true);
        await driver.wsSettingsWebhookToggleIndividual("Cycles");
        await driver.wsSettingsWebhookToggleIndividual("Modules");
        const flipped = await driver.wsSettingsWebhookIndividualOptions();
        expect(flipped.find((row) => row.label === "Cycles")?.checked).toBe(false);
        expect(flipped.find((row) => row.label === "Modules")?.checked).toBe(false);
        expect(flipped.find((row) => row.label === "Projects")?.checked).toBe(true);
      });

      await test.step("Cancel closes without saving", async () => {
        await driver.wsSettingsWebhookUrlFill(url);
        await driver.wsSettingsWebhookCancel();
        await expect.poll(() => driver.wsSettingsWebhookDialogOpen(), { timeout: 20_000 }).toBe(false);
        expect(await serverWebhooksList(seed.workspaceSlug, session)).toHaveLength(0);
      });

      await test.step("Escape closes without saving", async () => {
        await driver.wsSettingsWebhooksAddClick();
        await driver.wsSettingsWebhookUrlFill(url);
        await driver.wsSettingsWebhookPressEscape();
        await expect.poll(() => driver.wsSettingsWebhookDialogOpen(), { timeout: 20_000 }).toBe(false);
        expect(await serverWebhooksList(seed.workspaceSlug, session)).toHaveLength(0);
      });

      await test.step("success shows a notice and moves to the secret reveal", async () => {
        await driver.wsSettingsWebhooksAddClick();
        await driver.wsSettingsWebhookUrlFill(url);
        const file = await driver.wsSettingsWebhookSubmitAndCaptureDownload();
        expect(file.fileName).toMatch(/^webhook-secret-key-\d+\.csv$/);
        expect(await driver.wsSettingsWebhookRevealVisible()).toBe(true);
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Webhook created");
        const rows = await serverWebhooksList(seed.workspaceSlug, session);
        expect(rows).toHaveLength(1);
        createdId = rows[0]?.id ?? "";
        expect(createdId).not.toBe("");
        expect(rows[0]?.url).toBe(url);
        expect([rows[0]?.project, rows[0]?.cycle, rows[0]?.module, rows[0]?.issue, rows[0]?.issue_comment]).toEqual([
          true,
          true,
          true,
          true,
          true,
        ]);
        await driver.wsSettingsWebhookRevealClose();
        await expect.poll(() => driver.wsSettingsWebhookDialogOpen(), { timeout: 20_000 }).toBe(false);
      });

      await test.step("a duplicate URL fails with the dialog kept", async () => {
        await driver.wsSettingsWebhooksAddClick();
        await driver.wsSettingsWebhookUrlFill(url);
        await driver.wsSettingsWebhookSubmit();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Webhook not created");
        expect(await driver.wsSettingsWebhookDialogTitle()).toBe("Create webhook");
        expect(await driver.wsSettingsWebhookUrlValue()).toBe(url);
        await driver.wsSettingsWebhookCancel();
        expect(await serverWebhooksList(seed.workspaceSlug, session)).toHaveLength(1);
      });
    } finally {
      if (createdId !== "") await serverCleanupWebhook(seed.workspaceSlug, createdId, session);
    }
  }
);

test(
  specTitle(["WSET-021"], "the secret reveals once, copies, backs up to a file and clears on close"),
  { tag: specTags(["WSET-021"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    for (const row of await serverWebhooksList(seed.workspaceSlug, session)) {
      await serverCleanupWebhook(seed.workspaceSlug, row.id, session);
    }
    const url = webhookUrl("reveal");
    let createdId = "";
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
      await driver.wsSettingsWebhooksAddClick();
      await driver.wsSettingsWebhookUrlFill(url);
      const file = await driver.wsSettingsWebhookSubmitAndCaptureDownload();
      createdId = (await serverWebhooksList(seed.workspaceSlug, session)).find((row) => row.url === url)?.id ?? "";
      expect(createdId).not.toBe("");

      expect(await driver.wsSettingsWebhookRevealVisible()).toBe(true);
      const explainer = await driver.wsSettingsWebhookRevealText();
      expect(explainer).toContain("after you hit Close");
      expect(explainer).toContain("downloaded");

      await test.step("the key renders masked with show and copy actions", async () => {
        expect(await driver.wsSettingsWebhookSecretShown()).toBe(false);
        expect(await driver.wsSettingsWebhookSecretValue()).toBeNull();
        expect(await driver.wsSettingsWebhookSecretActionsVisible()).toBe(true);
      });

      await test.step("show and hide round-trip the in-clear key", async () => {
        await driver.wsSettingsWebhookSecretToggleView();
        expect(await driver.wsSettingsWebhookSecretShown()).toBe(true);
        const secret = await driver.wsSettingsWebhookSecretValue();
        expect(secret).not.toBeNull();
        expect(secret?.length ?? 0).toBeGreaterThan(20);
        await driver.wsSettingsWebhookSecretToggleView();
        expect(await driver.wsSettingsWebhookSecretShown()).toBe(false);
        expect(await driver.wsSettingsWebhookSecretValue()).toBeNull();
      });

      const secret = await (async () => {
        await driver.wsSettingsWebhookSecretToggleView();
        return (await driver.wsSettingsWebhookSecretValue()) ?? "";
      })();
      expect(secret).not.toBe("");

      await test.step("copy writes the key to the clipboard", async () => {
        await driver.wsSettingsWebhookSecretCopy();
        await expect.poll(() => driver.readClipboard(), { timeout: 20_000 }).toBe(secret);
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("copied");
      });

      await test.step("the backup file carries the URL and the key", async () => {
        expect(file.fileName).toMatch(/^webhook-secret-key-\d+\.csv$/);
        expect(file.content).toContain(url);
        expect(file.content).toContain(secret);
      });

      await test.step("closing clears the key: the detail page shows no actions", async () => {
        await driver.wsSettingsWebhookRevealClose();
        // Client-side navigation keeps the store alive, so an uncleared
        // key would still offer its actions here.
        await driver.wsSettingsWebhooksOpenRow(url);
        expect(await driver.wsSettingsWebhookSecretBlockVisible()).toBe(true);
        expect(await driver.wsSettingsWebhookSecretShown()).toBe(false);
        await expect.poll(() => driver.wsSettingsWebhookSecretActionsVisible(), { timeout: 20_000 }).toBe(false);
      });
    } finally {
      if (createdId !== "") await serverCleanupWebhook(seed.workspaceSlug, createdId, session);
    }
  }
);

test(
  specTitle(["WSET-022"], "webhook detail prefills, edits and reports save failures"),
  { tag: specTags(["WSET-022"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const created = await serverCreateWebhook(
      seed.workspaceSlug,
      { url: webhookUrl("detail"), project: true },
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);

      await test.step("a spinner covers the page while the fetch is held", async () => {
        await driver.wsSettingsWebhookDetailDelayFetch(3_000);
        const opened = driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, created.id);
        await expect.poll(() => driver.wsSettingsWebhookDetailLoaderVisible(), { timeout: 30_000 }).toBe(true);
        await opened;
      });

      await test.step("the form prefills URL, individual mode and the active switch", async () => {
        expect(await driver.wsSettingsWebhookUrlValue()).toBe(created.url);
        await expect.poll(() => driver.wsSettingsWebhookEventMode(), { timeout: 20_000 }).toBe("individual");
        const options = await driver.wsSettingsWebhookIndividualOptions();
        expect(options.find((row) => row.label === "Projects")?.checked).toBe(true);
        expect(options.find((row) => row.label === "Cycles")?.checked).toBe(false);
        expect(options.find((row) => row.label === "Modules")?.checked).toBe(false);
        expect(await driver.wsSettingsWebhookDetailActiveOn()).toBe(true);
      });

      await test.step("saving shows updating state then persists", async () => {
        const url2 = webhookUrl("Detail2");
        await driver.wsSettingsWebhookDetailDelayUpdate(3_000);
        await driver.wsSettingsWebhookUrlFill(url2);
        await driver.wsSettingsWebhookToggleIndividual("Modules");
        await driver.wsSettingsWebhookDetailToggleActive();
        const updated = driver.wsSettingsWebhookDetailUpdate();
        await expect.poll(() => driver.wsSettingsWebhookDetailUpdateLabel(), { timeout: 20_000 }).toContain("Updating");
        await updated;
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("Webhook updated");
        const details = await serverWebhookDetails(seed.workspaceSlug, created.id, session);
        expect(details.url).toBe(url2);
        expect(details.module).toBe(true);
        expect(details.is_active).toBe(false);
      });

      await test.step("a duplicate URL fails with the form kept", async () => {
        const other = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("other") }, session);
        try {
          await driver.wsSettingsWebhookUrlFill(other.url);
          await driver.wsSettingsWebhookDetailUpdate();
          await expect
            .poll(() => driver.lastToast(), { timeout: 20_000 })
            .toContain(failureNotice(driver, "Webhook not updated"));
          expect(await driver.wsSettingsWebhookUrlValue()).toBe(other.url);
          expect((await serverWebhookDetails(seed.workspaceSlug, created.id, session)).url).not.toBe(other.url);
        } finally {
          await serverCleanupWebhook(seed.workspaceSlug, other.id, session);
        }
      });
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);

test(
  specTitle(["WSET-022"], "members get the shared not-authorized view on webhook detail"),
  { tag: specTags(["WSET-022"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const created = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("deny") }, session);
    try {
      const member = requireMember(seed);
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, created.id);
      expect(await driver.wsSettingsWebhookDetailNotAuthorized()).toBe(true);
      expect(await driver.wsSettingsWebhookDetailDenialKind()).toBe("view");
      expect(await driver.wsSettingsWebhookSecretBlockVisible()).toBe(false);
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);

test(
  specTitle(["WSET-023"], "regenerate rotates the secret with loading state and one-time reveal"),
  { tag: specTags(["WSET-023"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const created = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("regen") }, session);
    expect(created.secret_key.length).toBeGreaterThan(0);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, created.id);
      expect(await driver.wsSettingsWebhookSecretBlockVisible()).toBe(true);
      expect(await driver.wsSettingsWebhookSecretShown()).toBe(false);
      expect(await driver.wsSettingsWebhookSecretActionsVisible()).toBe(false);

      await test.step("regenerating shows loading state then a success notice", async () => {
        await driver.wsSettingsWebhookDelayRegenerate(3_000);
        const done = driver.wsSettingsWebhookRegenerate();
        await expect
          .poll(() => driver.wsSettingsWebhookRegenerateLabel(), { timeout: 20_000 })
          .toContain("Re-generating");
        await done;
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("regenerated");
      });

      await test.step("the new key reveals once and differs from the previous one", async () => {
        expect(await driver.wsSettingsWebhookSecretActionsVisible()).toBe(true);
        expect(await driver.wsSettingsWebhookSecretShown()).toBe(false);
        await driver.wsSettingsWebhookSecretToggleView();
        const next = await driver.wsSettingsWebhookSecretValue();
        expect(next).not.toBeNull();
        expect(next ?? "").not.toBe("");
        expect(next).not.toBe(created.secret_key);
      });

      await test.step("regenerating a deleted webhook errors", async () => {
        const doomed = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("doomed") }, session);
        await driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, doomed.id);
        await serverCleanupWebhook(seed.workspaceSlug, doomed.id, session);
        await driver.wsSettingsWebhookRegenerate();
        await expect
          .poll(() => driver.lastToast(), { timeout: 20_000 })
          .toContain(failureNotice(driver, "Secret key not regenerated"));
      });
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);

test(
  specTitle(["WSET-023"], "bug: regenerate skips the backup download on a fresh load (NEWFRONT-419)"),
  { tag: specTags(["WSET-023"]) },
  async ({ driver, seed }) => {
    // Intended behavior (NEWFRONT-419): every successful regenerate
    // downloads a fresh backup file containing the NEW key. The old app
    // instead reads the key from a stale render closure: no download on
    // a fresh load, and the previous key in the file on a repeat.
    const session = await signInSession(seed.email, seed.password);
    const created = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("regenbug") }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, created.id);

      if (driver.target === "web_new") {
        // The new frontend builds the intended behavior: every successful
        // regenerate downloads a fresh file carrying the key it issued.
        const first = await driver.wsSettingsWebhookRegenerateAndCaptureDownload();
        expect(first).not.toBeNull();
        expect(first?.fileName).toMatch(/^webhook-secret-key-\d+\.csv$/);
        await driver.wsSettingsWebhookSecretToggleView();
        const issued = await driver.wsSettingsWebhookSecretValue();
        expect(issued).not.toBeNull();
        expect(issued).not.toBe(created.secret_key);
        expect(first?.content).toContain(issued ?? "");
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("regenerated");
        return;
      }

      // The old app's bug (NEWFRONT-419): no download on a fresh load (the
      // 10s absence probe outlives the success toast, so the toast is
      // asserted after the repeat below).
      expect(await driver.wsSettingsWebhookRegenerateAndCaptureDownload()).toBeNull();
      await driver.wsSettingsWebhookSecretToggleView();
      const second = await driver.wsSettingsWebhookSecretValue();
      expect(second).not.toBeNull();
      expect(second ?? "").not.toBe("");
      expect(second).not.toBe(created.secret_key);

      const stale = await driver.wsSettingsWebhookRegenerateAndCaptureDownload();
      expect(stale).not.toBeNull();
      expect(stale?.fileName).toMatch(/^webhook-secret-key-\d+\.csv$/);
      await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("regenerated");
      // The repeat download carries the previous key, not the new one.
      expect(stale?.content).toContain(second ?? "");
      const third = await driver.wsSettingsWebhookSecretValue();
      expect(third).not.toBeNull();
      expect(third).not.toBe(second);
      expect(stale?.content).not.toContain(third ?? "");
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);

test(
  specTitle(["WSET-024"], "delete webhook confirms, cancels, deletes and reports failures"),
  { tag: specTags(["WSET-024"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const created = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("delete") }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, created.id);
      expect(await driver.wsSettingsWebhookDeleteSectionVisible()).toBe(true);
      expect(await driver.wsSettingsWebhookDeletePanelVisible()).toBe(false);
      await driver.wsSettingsWebhookDeleteSectionExpand();
      expect(await driver.wsSettingsWebhookDeletePanelVisible()).toBe(true);

      await test.step("cancelling keeps the webhook", async () => {
        await driver.wsSettingsWebhookDeleteOpen();
        expect(await driver.wsSettingsWebhookDeleteDialogOpen()).toBe(true);
        expect(await driver.wsSettingsWebhookDeleteDialogText()).toContain("cannot be undone");
        await driver.wsSettingsWebhookDeleteCancel();
        expect(await driver.wsSettingsWebhookDeleteDialogOpen()).toBe(false);
        expect((await serverWebhookDetails(seed.workspaceSlug, created.id, session)).id).toBe(created.id);
      });

      await test.step("confirming deletes and returns to the list", async () => {
        await driver.wsSettingsWebhookDeleteOpen();
        await driver.wsSettingsWebhookDeleteConfirm();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("deleted");
        await expect.poll(() => driver.wsSettingsWebhooksAddVisible(), { timeout: 20_000 }).toBe(true);
        expect(await serverWebhooksList(seed.workspaceSlug, session)).toHaveLength(0);
      });

      await test.step("deleting an already-deleted webhook errors", async () => {
        const doomed = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("doomed") }, session);
        const doomedUrl = doomed.url;
        await driver.wsSettingsOpenWebhookDetail(seed.workspaceSlug, doomed.id);
        await serverCleanupWebhook(seed.workspaceSlug, doomed.id, session);
        await driver.wsSettingsWebhookDeleteSectionExpand();
        await driver.wsSettingsWebhookDeleteOpen();
        await driver.wsSettingsWebhookDeleteConfirm();
        await expect.poll(() => driver.lastToast(), { timeout: 20_000 }).toContain("could not be deleted");
        expect(await driver.wsSettingsWebhookUrlValue()).toBe(doomedUrl);
      });
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);

test(
  specTitle(["WSET-041"], "the list-row switch flips a webhook's active state"),
  { tag: specTags(["WSET-041"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const created = await serverCreateWebhook(seed.workspaceSlug, { url: webhookUrl("rowtoggle") }, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
      await expect.poll(() => driver.wsSettingsWebhooksRowUrls(), { timeout: 30_000 }).toContain(created.url);
      expect(await driver.wsSettingsWebhooksRowActiveOn(created.url)).toBe(true);

      await driver.wsSettingsWebhooksRowToggleActive(created.url);
      await expect
        .poll(async () => (await serverWebhookDetails(seed.workspaceSlug, created.id, session)).is_active, {
          timeout: 20_000,
        })
        .toBe(false);
      // The switch sits inside the row link: reopen to read UI state.
      await driver.wsSettingsOpenWebhooks(seed.workspaceSlug);
      await expect.poll(() => driver.wsSettingsWebhooksRowActiveOn(created.url), { timeout: 30_000 }).toBe(false);

      await driver.wsSettingsWebhooksRowToggleActive(created.url);
      await expect
        .poll(async () => (await serverWebhookDetails(seed.workspaceSlug, created.id, session)).is_active, {
          timeout: 20_000,
        })
        .toBe(true);
    } finally {
      await serverCleanupWebhook(seed.workspaceSlug, created.id, session);
    }
  }
);
