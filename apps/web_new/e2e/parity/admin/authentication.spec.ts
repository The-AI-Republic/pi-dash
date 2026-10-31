// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-443): instance-admin authentication landing
// page and per-provider OAuth sub-pages. Rows ADM-023 (open-sign-up
// toggle), ADM-024 (method registry fixed order), ADM-025 (per-method
// enable plus last-method guard), ADM-026 (card adapts to
// configured/unconfigured), ADM-027 (provider sub-page shell plus master
// toggle), ADM-028 (GitHub form), ADM-029 (GitLab form), ADM-030 (Gitea
// form), ADM-031 (Google form), ADM-032 (sync toggle vs enable toggle),
// ADM-033 (copy-to-clipboard URL fields), ADM-034 (unsaved-changes
// discard guard), ADM-035 (save gating plus reset-to-server) and ADM-036
// (landing-page skeleton).
//
// Observed behavior notes (paraphrased; the inventory rows carry them at
// update time): the landing page saves each flip as a single-key write
// with a progress notice that settles into success or failure, and every
// switch on the page locks while any write is in flight. Disabling the
// last enabled method is refused client-side with an error notice and no
// request; the guard counts only the six method keys, so the sign-up
// toggle flips freely. OAuth cards offer a configure link until their
// credentials land (Gitea also needs a host), then an edit link plus a
// toggle. Each provider sub-page carries a master toggle that persists
// its enable key with a provider-specific notice, over a credential form
// that saves all its fields at once, resets to the server response, and
// intercepts leaving while dirty with a confirm-discard modal. The sync
// switch only stages its string flag until the form saves. Copy fields
// write the shown URL to the clipboard with an info notice; callbacks
// derive from the origin plus the provider path, and the Gitea help link
// follows the typed host as of the last form render (a cleared host
// reads stale until the next render, then falls back to the default
// host; bug: NEWFRONT-492 — the new console follows the draft live). An
// unfinished required field blocks the
// submit silently.
// Loading shows five-bar skeletons on both the landing and the provider
// body.
import { test, expect } from "../fixtures";
import { serverAdminGetConfigurations, serverAdminSession, serverAdminUpdateConfigurations } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";

/** Configuration keys this spec snapshots and restores. */
const AUTH_KEYS = [
  "ENABLE_SIGNUP",
  "ENABLE_MAGIC_LINK_LOGIN",
  "ENABLE_EMAIL_PASSWORD",
  "IS_GOOGLE_ENABLED",
  "GOOGLE_CLIENT_ID",
  "GOOGLE_CLIENT_SECRET",
  "ENABLE_GOOGLE_SYNC",
  "IS_GITHUB_ENABLED",
  "GITHUB_CLIENT_ID",
  "GITHUB_CLIENT_SECRET",
  "GITHUB_ORGANIZATION_ID",
  "ENABLE_GITHUB_SYNC",
  "IS_GITLAB_ENABLED",
  "GITLAB_HOST",
  "GITLAB_CLIENT_ID",
  "GITLAB_CLIENT_SECRET",
  "ENABLE_GITLAB_SYNC",
  "IS_GITEA_ENABLED",
  "GITEA_HOST",
  "GITEA_CLIENT_ID",
  "GITEA_CLIENT_SECRET",
  "ENABLE_GITEA_SYNC",
] as const;

/** Method cards in the registry's fixed order. */
const METHOD_ORDER = ["Unique codes", "Passwords", "Google", "GitHub", "GitLab", "Gitea"] as const;

/** Landing card descriptions as rendered. */
const METHOD_DESCRIPTIONS: Record<string, string> = {
  "Unique codes":
    "Log in or sign up for Pi Dash using codes sent via email. You need to have set up SMTP to use this method.",
  Passwords: "Allow members to create accounts with passwords and use it with their email addresses to sign in.",
  Google: "Allow members to log in or sign up for Pi Dash with their Google accounts.",
  GitHub: "Allow members to log in or sign up for Pi Dash with their GitHub accounts.",
  GitLab: "Allow members to log in or sign up to pi dash with their GitLab accounts.",
  Gitea: "Allow members to log in or sign up to pi dash with their Gitea accounts.",
};

/** Per-provider sub-page facts as rendered. */
const PROVIDERS = [
  {
    key: "github",
    name: "GitHub",
    enableKey: "IS_GITHUB_ENABLED",
    title: "GitHub Authentication - God Mode",
    description: "Allow members to login or sign up to pi dash with their GitHub accounts.",
  },
  {
    key: "gitlab",
    name: "GitLab",
    enableKey: "IS_GITLAB_ENABLED",
    title: "GitLab Authentication - God Mode",
    description: "Allow members to login or sign up to pi dash with their GitLab accounts.",
  },
  {
    key: "gitea",
    name: "Gitea",
    enableKey: "IS_GITEA_ENABLED",
    title: "Gitea Authentication - God Mode",
    description: "Allow members to login or sign up to pi dash with their Gitea accounts.",
  },
  {
    key: "google",
    name: "Google",
    enableKey: "IS_GOOGLE_ENABLED",
    title: "Google Authentication - God Mode",
    description: "Allow members to login or sign up to pi dash with their Google accounts.",
  },
] as const;

/** Local sleep for proving an absence (no write, no notice) over a window. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Snapshot the auth rows this spec may rewrite, for teardown restore. */
async function snapshotAuth(session: string): Promise<Record<string, string>> {
  const all = await serverAdminGetConfigurations(session);
  return Object.fromEntries(AUTH_KEYS.map((key) => [key, all[key] ?? ""]));
}

/** Restore auth rows and prove the restore landed (keeps retries convergent). */
async function restoreAuth(session: string, snap: Record<string, string>): Promise<void> {
  await serverAdminUpdateConfigurations(session, snap);
  const after = await serverAdminGetConfigurations(session);
  for (const key of AUTH_KEYS) {
    expect(after[key]).toBe(snap[key]);
  }
}

/** Open the authentication landing page signed in, waiting for it to load. */
async function openAuth(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await driver.adminOpenPath("/authentication/");
  await driver.adminWaitForRootBranch("dashboard");
  await expect.poll(async () => (await driver.adminAuthLandingFacts()).loaded, { timeout: 15_000 }).toBe(true);
}

/** Open a provider sub-page signed in, waiting for its form to render. */
async function openProvider(driver: ParityDriver, seed: ParitySeedFacts, provider: string): Promise<void> {
  await driver.adminEnsureSignedIn(seed.email, seed.password);
  await gotoProvider(driver, provider);
}

/** Navigate to a provider sub-page on an already-signed-in session. */
async function gotoProvider(driver: ParityDriver, provider: string): Promise<void> {
  await driver.adminOpenPath(`/authentication/${provider}/`);
  await driver.adminWaitForRootBranch("dashboard");
  await expect.poll(async () => (await driver.adminAuthProviderFacts()).formVisible, { timeout: 15_000 }).toBe(true);
}

test(
  specTitle(["ADM-023"], "open-sign-up toggle persists with saving then success"),
  { tag: specTags(["ADM-023"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, { ENABLE_SIGNUP: "1" });
      await openAuth(driver, seed);
      expect((await driver.adminAuthLandingFacts()).signupOn).toBe(true);

      // Flipping off persists the single key with a success notice.
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthSetSignup(false);
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([{ ENABLE_SIGNUP: "0" }]);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Success",
          message: "Configuration saved successfully",
        });
      expect((await serverAdminGetConfigurations(session))["ENABLE_SIGNUP"]).toBe("0");
      expect((await driver.adminAuthLandingFacts()).signupOn).toBe(false);

      // And back on the same way.
      await driver.adminAuthSetSignup(true);
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(2);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Success",
          message: "Configuration saved successfully",
        });
      expect((await serverAdminGetConfigurations(session))["ENABLE_SIGNUP"]).toBe("1");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-023"], "in-flight save locks the switch group and shows the progress notice"),
  { tag: specTags(["ADM-023"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, { ENABLE_SIGNUP: "1" });
      await openAuth(driver, seed);
      await driver.adminStartConfigPatchSpy();
      await driver.adminDelayConfigPatch(3000);
      try {
        await driver.adminAuthSetSignup(false);
        // While the write hangs, the progress notice shows and every
        // switch on the page is disabled.
        await expect
          .poll(async () => (await driver.adminLastToast())?.title ?? "", { timeout: 15_000 })
          .toContain("Saving configuration");
        // The lock is a steady property of the held write, so read it as
        // one: a single sampling can land between the progress notice and
        // the re-render that disables the group.
        await expect
          .poll(
            async () => {
              const facts = await driver.adminAuthLandingFacts();
              return {
                signupDisabled: facts.signupDisabled,
                cardsLocked: facts.cards.filter((card) => card.toggleOn !== null).every((card) => card.toggleDisabled),
              };
            },
            { timeout: 15_000 }
          )
          .toEqual({ signupDisabled: true, cardsLocked: true });
        // The held write still lands once the delay lifts.
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        await expect
          .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Success",
            message: "Configuration saved successfully",
          });
        expect((await driver.adminAuthLandingFacts()).signupDisabled).toBe(false);
        expect((await serverAdminGetConfigurations(session))["ENABLE_SIGNUP"]).toBe("0");
      } finally {
        await driver.adminClearConfigPatchFault();
      }
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-023"], "failed sign-up save reports failure and keeps the stored value"),
  { tag: specTags(["ADM-023"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, { ENABLE_SIGNUP: "1" });
      await openAuth(driver, seed);
      await driver.adminStartConfigPatchSpy();
      await driver.adminStartConsoleSpy();
      await driver.adminFailConfigPatch();
      try {
        await driver.adminAuthSetSignup(false);
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        await expect
          .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Error",
            message: "Failed to save configuration",
          });
        await expect
          .poll(
            async () =>
              (await driver.adminConsoleErrors()).some((line) =>
                line.includes("Error updating the instance configurations")
              ),
            { timeout: 15_000 }
          )
          .toBe(true);
        // The server kept the stored value and the switch shows it again.
        expect((await serverAdminGetConfigurations(session))["ENABLE_SIGNUP"]).toBe("1");
        const facts = await driver.adminAuthLandingFacts();
        expect(facts.signupOn).toBe(true);
        expect(facts.signupDisabled).toBe(false);
      } finally {
        await driver.adminClearConfigPatchFault();
      }
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-024"], "method cards render in fixed order with icon, name and description"),
  { tag: specTags(["ADM-024"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openAuth(driver, seed);
    const facts = await driver.adminAuthLandingFacts();
    expect(facts.cards.map((card) => card.name)).toEqual([...METHOD_ORDER]);
    for (const card of facts.cards) {
      expect(card.hasIcon).toBe(true);
      expect(card.description).toBe(METHOD_DESCRIPTIONS[card.name]);
    }
    // The magic-link card notes the SMTP prerequisite.
    expect(facts.cards[0]?.description ?? "").toContain("SMTP");
  }
);

test(
  specTitle(["ADM-025"], "method toggle persists its enable key"),
  { tag: specTags(["ADM-025"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        ENABLE_MAGIC_LINK_LOGIN: "0",
        ENABLE_EMAIL_PASSWORD: "1",
      });
      await openAuth(driver, seed);
      const cards = (await driver.adminAuthLandingFacts()).cards;
      expect(cards.find((card) => card.name === "Unique codes")?.toggleOn).toBe(false);

      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthSetMethodEnabled("Unique codes", true);
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([{ ENABLE_MAGIC_LINK_LOGIN: "1" }]);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Success",
          message: "Configuration saved successfully",
        });
      expect((await serverAdminGetConfigurations(session))["ENABLE_MAGIC_LINK_LOGIN"]).toBe("1");
      const after = (await driver.adminAuthLandingFacts()).cards;
      expect(after.find((card) => card.name === "Unique codes")?.toggleOn).toBe(true);
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-025"], "disabling the last method is blocked; sign-up stays exempt"),
  { tag: specTags(["ADM-025"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        ENABLE_SIGNUP: "1",
        ENABLE_MAGIC_LINK_LOGIN: "1",
        ENABLE_EMAIL_PASSWORD: "0",
        IS_GOOGLE_ENABLED: "0",
        IS_GITHUB_ENABLED: "0",
        IS_GITLAB_ENABLED: "0",
        IS_GITEA_ENABLED: "0",
      });
      await openAuth(driver, seed);

      // The lone enabled method refuses to switch off: an error notice,
      // no request, and the server untouched.
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthSetMethodEnabled("Unique codes", false);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Cannot disable authentication",
          message:
            "At least one authentication method must remain enabled. Please enable another method before disabling this one.",
        });
      await sleep(1500);
      expect(await driver.adminConfigPatchBodies()).toEqual([]);
      expect((await serverAdminGetConfigurations(session))["ENABLE_MAGIC_LINK_LOGIN"]).toBe("1");
      const kept = (await driver.adminAuthLandingFacts()).cards;
      expect(kept.find((card) => card.name === "Unique codes")?.toggleOn).toBe(true);

      // The guard counts only the six method keys: sign-up flips freely.
      await driver.adminAuthSetSignup(false);
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([{ ENABLE_SIGNUP: "0" }]);

      // With a second method on, the first one switches off normally.
      await driver.adminAuthSetMethodEnabled("Passwords", true);
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(2);
      await driver.adminAuthSetMethodEnabled("Unique codes", false);
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(3);
      const bodies = await driver.adminConfigPatchBodies();
      expect(bodies[1]).toEqual({ ENABLE_EMAIL_PASSWORD: "1" });
      expect(bodies[2]).toEqual({ ENABLE_MAGIC_LINK_LOGIN: "0" });
      const stored = await serverAdminGetConfigurations(session);
      expect(stored["ENABLE_MAGIC_LINK_LOGIN"]).toBe("0");
      expect(stored["ENABLE_EMAIL_PASSWORD"]).toBe("1");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-026"], "OAuth cards trade configure for edit plus toggle once credentials land"),
  { tag: specTags(["ADM-026"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    const byName = async (name: string) =>
      (await driver.adminAuthLandingFacts()).cards.find((card) => card.name === name);
    try {
      await serverAdminUpdateConfigurations(session, {
        GITHUB_CLIENT_ID: "",
        GITHUB_CLIENT_SECRET: "",
        IS_GITHUB_ENABLED: "0",
        GITLAB_CLIENT_ID: "",
        GITLAB_CLIENT_SECRET: "",
        GOOGLE_CLIENT_ID: "",
        GOOGLE_CLIENT_SECRET: "",
        GITEA_HOST: "",
        GITEA_CLIENT_ID: "",
        GITEA_CLIENT_SECRET: "",
      });
      await openAuth(driver, seed);

      // Magic-link and password cards are toggle-only; every
      // unconfigured OAuth card offers only a configure link.
      expect((await byName("Unique codes"))?.control).toBe("toggle");
      expect((await byName("Passwords"))?.control).toBe("toggle");
      for (const name of ["Google", "GitHub", "GitLab", "Gitea"]) {
        const card = await byName(name);
        expect(card?.control).toBe("configure");
        expect(card?.toggleOn).toBeNull();
        expect(card?.linkHref ?? "").toMatch(new RegExp(`/authentication/${name.toLowerCase()}/$`));
      }

      // Credentials turn the GitHub card into edit plus a live toggle.
      await serverAdminUpdateConfigurations(session, {
        GITHUB_CLIENT_ID: "parity-github-id",
        GITHUB_CLIENT_SECRET: "parity-github-secret",
        IS_GITHUB_ENABLED: "1",
      });
      await driver.adminOpenPath("/authentication/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminAuthLandingFacts()).loaded, { timeout: 15_000 }).toBe(true);
      const github = await byName("GitHub");
      expect(github?.control).toBe("edit-toggle");
      expect(github?.toggleOn).toBe(true);
      expect(github?.linkHref ?? "").toMatch(/\/authentication\/github\/$/);

      // Gitea also requires a host: credentials alone leave it unconfigured.
      await serverAdminUpdateConfigurations(session, {
        GITEA_CLIENT_ID: "parity-gitea-id",
        GITEA_CLIENT_SECRET: "parity-gitea-secret",
      });
      await driver.adminOpenPath("/authentication/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminAuthLandingFacts()).loaded, { timeout: 15_000 }).toBe(true);
      expect((await byName("Gitea"))?.control).toBe("configure");
      await serverAdminUpdateConfigurations(session, { GITEA_HOST: "https://gitea.parity.invalid" });
      await driver.adminOpenPath("/authentication/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect.poll(async () => (await driver.adminAuthLandingFacts()).loaded, { timeout: 15_000 }).toBe(true);
      const gitea = await byName("Gitea");
      expect(gitea?.control).toBe("edit-toggle");
      expect(gitea?.linkHref ?? "").toMatch(/\/authentication\/gitea\/$/);
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-026"], "GitHub logo swaps with the light/dark theme"),
  { tag: specTags(["ADM-026"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openAuth(driver, seed);
    await driver.adminSetTheme("light");
    const light = await driver.adminAuthMethodIconSrc("GitHub");
    expect(light ?? "").toContain("github-black");
    await driver.adminSetTheme("dark");
    const dark = await driver.adminAuthMethodIconSrc("GitHub");
    expect(dark ?? "").toContain("github-white");
    expect(dark).not.toBe(light);
    // The lucide-icon cards expose no logo image.
    expect(await driver.adminAuthMethodIconSrc("Unique codes")).toBeNull();
    expect(await driver.adminAuthMethodIconSrc("Passwords")).toBeNull();
  }
);

test(
  specTitle(["ADM-027"], "provider shell shows its identity and persists the master toggle"),
  { tag: specTags(["ADM-027"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      for (const [index, provider] of PROVIDERS.entries()) {
        await serverAdminUpdateConfigurations(session, { [provider.enableKey]: "0" });
        if (index === 0) await openProvider(driver, seed, provider.key);
        else await gotoProvider(driver, provider.key);
        const shell = await driver.adminAuthProviderFacts();
        expect(shell.name).toBe(provider.name);
        expect(shell.description).toBe(provider.description);
        expect(shell.hasIcon).toBe(true);
        expect(shell.title).toBe(provider.title);
        expect(shell.toggleOn).toBe(false);
        expect(shell.toggleDisabled).toBe(false);
        expect(shell.formVisible).toBe(true);
        expect(shell.skeletonVisible).toBe(false);

        await driver.adminStartConfigPatchSpy();
        await driver.adminAuthProviderSetEnabled(true);
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        expect(await driver.adminConfigPatchBodies()).toEqual([{ [provider.enableKey]: "1" }]);
        await expect
          .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Configuration saved",
            message: `${provider.name} authentication is now active.`,
          });
        expect((await serverAdminGetConfigurations(session))[provider.enableKey]).toBe("1");
        expect((await driver.adminAuthProviderFacts()).toggleOn).toBe(true);

        await driver.adminAuthProviderSetEnabled(false);
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(2);
        await expect
          .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Configuration saved",
            message: `${provider.name} authentication is now disabled.`,
          });
        expect((await serverAdminGetConfigurations(session))[provider.enableKey]).toBe("0");
      }
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-027"], "master toggle locks while its save is in flight"),
  { tag: specTags(["ADM-027"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, { IS_GITHUB_ENABLED: "0" });
      await openProvider(driver, seed, "github");
      await driver.adminStartConfigPatchSpy();
      await driver.adminDelayConfigPatch(3000);
      try {
        await driver.adminAuthProviderSetEnabled(true);
        await expect
          .poll(async () => (await driver.adminAuthProviderFacts()).toggleDisabled, { timeout: 15_000 })
          .toBe(true);
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        await expect
          .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Configuration saved",
            message: "GitHub authentication is now active.",
          });
        expect((await driver.adminAuthProviderFacts()).toggleDisabled).toBe(false);
        expect((await serverAdminGetConfigurations(session))["IS_GITHUB_ENABLED"]).toBe("1");
      } finally {
        await driver.adminClearConfigPatchFault();
      }
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-027"], "shell renders disabled over a body skeleton before config loads"),
  { tag: specTags(["ADM-027"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await driver.adminEnsureSignedIn(seed.email, seed.password);
      // The new console compiles the provider page on first visit, so a
      // cold load can outlast a short delay; hold the read long enough
      // for the skeleton window to be observable there too.
      await driver.adminDelayConfigFetch(driver.target === "web_new" ? 120_000 : 4000);
      try {
        await driver.adminOpenPath("/authentication/github/");
        // The header shows with a locked toggle while the body is only a skeleton.
        await expect
          .poll(
            async () => {
              const shell = await driver.adminAuthProviderFacts();
              return shell.name === "GitHub" &&
                shell.toggleDisabled &&
                shell.skeletonVisible &&
                shell.skeletonBars === 5 &&
                !shell.formVisible
                ? "skeleton-only"
                : "waiting";
            },
            { timeout: 15_000 }
          )
          .toBe("skeleton-only");
      } finally {
        await driver.adminClearConfigFetchFault();
      }
      await expect
        .poll(async () => (await driver.adminAuthProviderFacts()).formVisible, { timeout: 15_000 })
        .toBe(true);
      const shell = await driver.adminAuthProviderFacts();
      expect(shell.toggleDisabled).toBe(false);
      expect(shell.skeletonVisible).toBe(false);
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-028"], "GitHub credentials persist with the sync flag"),
  { tag: specTags(["ADM-028"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GITHUB_CLIENT_ID: "",
        GITHUB_CLIENT_SECRET: "",
        GITHUB_ORGANIZATION_ID: "",
        ENABLE_GITHUB_SYNC: "0",
      });
      await openProvider(driver, seed, "github");
      let form = await driver.adminAuthFormFacts();
      expect(form.values).toMatchObject({
        GITHUB_CLIENT_ID: "",
        GITHUB_CLIENT_SECRET: "",
        GITHUB_ORGANIZATION_ID: "",
      });
      expect(form.types["GITHUB_CLIENT_ID"]).toBe("text");
      expect(form.types["GITHUB_CLIENT_SECRET"]).toBe("password");
      expect(form.syncOn).toBe(false);
      expect(form.syncLabel).toBe("Refresh user attributes from GitHub during sign in");
      expect(form.saveDisabled).toBe(true);
      expect(form.saveLabel).toBe("Save changes");
      expect(form.backHref ?? "").toMatch(/\/authentication\/$/);

      await driver.adminAuthFormFill({
        GITHUB_CLIENT_ID: "parity-github-id",
        GITHUB_CLIENT_SECRET: "parity-github-secret",
        GITHUB_ORGANIZATION_ID: "424242",
      });
      await driver.adminAuthFormSetSync(true);
      expect((await driver.adminAuthFormFacts()).saveDisabled).toBe(false);

      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormSave();
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([
        {
          GITHUB_CLIENT_ID: "parity-github-id",
          GITHUB_CLIENT_SECRET: "parity-github-secret",
          GITHUB_ORGANIZATION_ID: "424242",
          ENABLE_GITHUB_SYNC: "1",
        },
      ]);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Done!",
          message: "Your GitHub authentication is configured. You should test it now.",
        });
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["GITHUB_CLIENT_ID"]).toBe("parity-github-id");
      expect(saved["GITHUB_CLIENT_SECRET"]).toBe("parity-github-secret");
      expect(saved["GITHUB_ORGANIZATION_ID"]).toBe("424242");
      expect(saved["ENABLE_GITHUB_SYNC"]).toBe("1");

      // A fresh load shows the saved values with the form clean again.
      await driver.adminOpenPath("/authentication/github/");
      await driver.adminWaitForRootBranch("dashboard");
      await expect
        .poll(async () => (await driver.adminAuthProviderFacts()).formVisible, { timeout: 15_000 })
        .toBe(true);
      form = await driver.adminAuthFormFacts();
      expect(form.values["GITHUB_CLIENT_ID"]).toBe("parity-github-id");
      expect(form.values["GITHUB_ORGANIZATION_ID"]).toBe("424242");
      expect(form.syncOn).toBe(true);
      expect(form.saveDisabled).toBe(true);
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-029"], "GitLab credentials persist with the host"),
  { tag: specTags(["ADM-029"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GITLAB_HOST: "",
        GITLAB_CLIENT_ID: "",
        GITLAB_CLIENT_SECRET: "",
        ENABLE_GITLAB_SYNC: "0",
      });
      await openProvider(driver, seed, "gitlab");
      const form = await driver.adminAuthFormFacts();
      expect(form.values).toMatchObject({ GITLAB_HOST: "", GITLAB_CLIENT_ID: "", GITLAB_CLIENT_SECRET: "" });
      expect(form.placeholders["GITLAB_HOST"]).toBe("https://gitlab.com");
      expect(form.types["GITLAB_CLIENT_SECRET"]).toBe("password");
      expect(form.syncLabel).toBe("Refresh user attributes from GitLab during sign in");

      await driver.adminAuthFormFill({
        GITLAB_HOST: "https://gitlab.parity.invalid",
        GITLAB_CLIENT_ID: "parity-gitlab-id",
        GITLAB_CLIENT_SECRET: "parity-gitlab-secret",
      });
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormSave();
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([
        {
          GITLAB_HOST: "https://gitlab.parity.invalid",
          GITLAB_CLIENT_ID: "parity-gitlab-id",
          GITLAB_CLIENT_SECRET: "parity-gitlab-secret",
          ENABLE_GITLAB_SYNC: "0",
        },
      ]);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Done!",
          message: "Your GitLab authentication is configured. You should test it now.",
        });
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["GITLAB_HOST"]).toBe("https://gitlab.parity.invalid");
      expect(saved["GITLAB_CLIENT_ID"]).toBe("parity-gitlab-id");
      expect(saved["GITLAB_CLIENT_SECRET"]).toBe("parity-gitlab-secret");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-030"], "bug: NEWFRONT-492 Gitea credentials persist; the help link lags the typed host"),
  { tag: specTags(["ADM-030"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GITEA_HOST: "",
        GITEA_CLIENT_ID: "",
        GITEA_CLIENT_SECRET: "",
        ENABLE_GITEA_SYNC: "0",
      });
      await openProvider(driver, seed, "gitea");

      // An unset host shows the public default, and the callback help
      // link points at the default host's application settings.
      expect((await driver.adminAuthFormFacts()).values["GITEA_HOST"]).toBe("https://gitea.com");
      expect(await driver.adminAuthCopyHelpHref("Callback URI")).toBe("https://gitea.com/user/settings/applications");

      // Typing a host repoints the help link once the form re-renders.
      await driver.adminAuthFormFill({ GITEA_HOST: "https://gitea.parity.invalid" });
      await expect
        .poll(async () => driver.adminAuthCopyHelpHref("Callback URI"), { timeout: 15_000 })
        .toBe("https://gitea.parity.invalid/user/settings/applications");

      // Clearing the host: the oracle keeps the old host's link until the
      // next render (NEWFRONT-492 bug); the new console follows the draft
      // and falls back to the public host at once.
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormFill({ GITEA_HOST: "" });
      if (driver.target === "web_new") {
        await expect
          .poll(async () => driver.adminAuthCopyHelpHref("Callback URI"), { timeout: 15_000 })
          .toBe("https://gitea.com/user/settings/applications");
      } else {
        expect(await driver.adminAuthCopyHelpHref("Callback URI")).toBe(
          "https://gitea.parity.invalid/user/settings/applications"
        );
      }
      await driver.adminAuthFormSave();
      await sleep(1500);
      expect(await driver.adminConfigPatchBodies()).toEqual([]);
      await expect
        .poll(async () => driver.adminAuthCopyHelpHref("Callback URI"), { timeout: 15_000 })
        .toBe("https://gitea.com/user/settings/applications");

      await driver.adminAuthFormFill({
        GITEA_HOST: "https://gitea.parity.invalid",
        GITEA_CLIENT_ID: "parity-gitea-id",
        GITEA_CLIENT_SECRET: "parity-gitea-secret",
      });
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormSave();
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([
        {
          GITEA_HOST: "https://gitea.parity.invalid",
          GITEA_CLIENT_ID: "parity-gitea-id",
          GITEA_CLIENT_SECRET: "parity-gitea-secret",
          ENABLE_GITEA_SYNC: "0",
        },
      ]);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Done!",
          message: "Your Gitea authentication is configured. You should test it now.",
        });
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["GITEA_HOST"]).toBe("https://gitea.parity.invalid");
      expect(saved["GITEA_CLIENT_ID"]).toBe("parity-gitea-id");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-031"], "Google credentials persist"),
  { tag: specTags(["ADM-031"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GOOGLE_CLIENT_ID: "",
        GOOGLE_CLIENT_SECRET: "",
        ENABLE_GOOGLE_SYNC: "0",
      });
      await openProvider(driver, seed, "google");
      const form = await driver.adminAuthFormFacts();
      expect(form.values).toMatchObject({ GOOGLE_CLIENT_ID: "", GOOGLE_CLIENT_SECRET: "" });
      expect(form.types["GOOGLE_CLIENT_SECRET"]).toBe("password");
      expect(form.syncLabel).toBe("Refresh user attributes from Google during sign in");
      expect((await driver.adminAuthCopyFacts()).map((field) => field.label)).toEqual(["Origin URL", "Callback URI"]);

      await driver.adminAuthFormFill({
        GOOGLE_CLIENT_ID: "parity-google-id",
        GOOGLE_CLIENT_SECRET: "parity-google-secret",
      });
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormSave();
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([
        {
          GOOGLE_CLIENT_ID: "parity-google-id",
          GOOGLE_CLIENT_SECRET: "parity-google-secret",
          ENABLE_GOOGLE_SYNC: "0",
        },
      ]);
      await expect
        .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
        .toEqual({
          title: "Done!",
          message: "Your Google authentication is configured. You should test it now.",
        });
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["GOOGLE_CLIENT_ID"]).toBe("parity-google-id");
      expect(saved["GOOGLE_CLIENT_SECRET"]).toBe("parity-google-secret");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-032"], "sync switch stages its flag until the form saves, apart from the master toggle"),
  { tag: specTags(["ADM-032"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        IS_GITHUB_ENABLED: "0",
        GITHUB_CLIENT_ID: "parity-sync-id",
        GITHUB_CLIENT_SECRET: "parity-sync-secret",
        GITHUB_ORGANIZATION_ID: "",
        ENABLE_GITHUB_SYNC: "0",
      });
      await openProvider(driver, seed, "github");
      expect((await driver.adminAuthFormFacts()).syncOn).toBe(false);

      // The flip alone writes nothing, though it dirties the form.
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormSetSync(true);
      expect((await driver.adminAuthFormFacts()).syncOn).toBe(true);
      expect((await driver.adminAuthFormFacts()).saveDisabled).toBe(false);
      await sleep(1500);
      expect(await driver.adminConfigPatchBodies()).toEqual([]);
      expect((await serverAdminGetConfigurations(session))["ENABLE_GITHUB_SYNC"]).toBe("0");

      // Only the save persists the flag, as its string value.
      await driver.adminAuthFormSave();
      await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
      expect(await driver.adminConfigPatchBodies()).toEqual([
        {
          GITHUB_CLIENT_ID: "parity-sync-id",
          GITHUB_CLIENT_SECRET: "parity-sync-secret",
          GITHUB_ORGANIZATION_ID: "",
          ENABLE_GITHUB_SYNC: "1",
        },
      ]);
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["ENABLE_GITHUB_SYNC"]).toBe("1");

      // The master enable toggle sat out the whole exchange.
      expect(saved["IS_GITHUB_ENABLED"]).toBe("0");
      expect((await driver.adminAuthProviderFacts()).toggleOn).toBe(false);
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-033"], "copy fields write their URL to the clipboard with a notice"),
  { tag: specTags(["ADM-033"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    await openProvider(driver, seed, "github");
    const copies = await driver.adminAuthCopyFacts();
    expect(copies.map((field) => field.label)).toEqual(["Origin URL", "Callback URI"]);
    const origin = copies[0]?.url ?? "";
    expect(origin.startsWith("http")).toBe(true);
    expect(copies[1]?.url).toBe(`${origin}/auth/github/callback/`);

    await driver.adminAuthCopyField("Callback URI");
    await expect.poll(async () => driver.adminAuthReadClipboard(), { timeout: 15_000 }).toBe(copies[1]?.url);
    await expect
      .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
      .toEqual({
        title: "Copied to clipboard",
        message: "The Callback URI has been successfully copied to your clipboard",
      });

    await driver.adminAuthCopyField("Origin URL");
    await expect.poll(async () => driver.adminAuthReadClipboard(), { timeout: 15_000 }).toBe(origin);

    // GitLab carries a single callback field on the same derivation.
    await gotoProvider(driver, "gitlab");
    const gitlab = await driver.adminAuthCopyFacts();
    expect(gitlab.map((field) => field.label)).toEqual(["Callback URL"]);
    expect(gitlab[0]?.url).toBe(`${origin}/auth/gitlab/callback/`);
    await driver.adminAuthCopyField("Callback URL");
    await expect.poll(async () => driver.adminAuthReadClipboard(), { timeout: 15_000 }).toBe(gitlab[0]?.url);
    await expect
      .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
      .toEqual({
        title: "Copied to clipboard",
        message: "The Callback URL has been successfully copied to your clipboard",
      });
  }
);

test(
  specTitle(["ADM-034"], "leaving dirty asks to discard; leaving clean goes straight back"),
  { tag: specTags(["ADM-034"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GITHUB_CLIENT_ID: "parity-keep-id",
        GITHUB_CLIENT_SECRET: "parity-keep-secret",
        GITHUB_ORGANIZATION_ID: "",
        ENABLE_GITHUB_SYNC: "0",
      });
      await openProvider(driver, seed, "github");
      expect((await driver.adminAuthDiscardFacts()).open).toBe(false);

      // A clean form goes straight back to the landing page.
      await driver.adminAuthFormBack();
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 15_000 }).toMatch(/\/authentication\/$/);

      // A dirty form is intercepted by the confirm-discard modal.
      await gotoProvider(driver, "github");
      await driver.adminAuthFormFill({ GITHUB_CLIENT_ID: "parity-dirty-id" });
      await driver.adminAuthFormBack();
      await expect.poll(async () => (await driver.adminAuthDiscardFacts()).open, { timeout: 15_000 }).toBe(true);
      const modal = await driver.adminAuthDiscardFacts();
      expect(modal).toMatchObject({
        open: true,
        title: "You have unsaved changes",
        body: "Changes you made will be lost if you go back. Do you wish to go back?",
        keepLabel: "Keep editing",
        backLabel: "Go back",
      });
      expect(modal.backHref ?? "").toMatch(/\/authentication\/$/);

      // Keeping the edits shuts the modal and preserves the draft.
      await driver.adminAuthDiscardKeep();
      await expect.poll(async () => (await driver.adminAuthDiscardFacts()).open, { timeout: 15_000 }).toBe(false);
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 15_000 }).toMatch(/\/github\/$/);
      expect((await driver.adminAuthFormFacts()).values["GITHUB_CLIENT_ID"]).toBe("parity-dirty-id");

      // Leaving discards the draft and lands on the landing page.
      await driver.adminAuthFormBack();
      await expect.poll(async () => (await driver.adminAuthDiscardFacts()).open, { timeout: 15_000 }).toBe(true);
      await driver.adminAuthDiscardLeave();
      await expect.poll(async () => driver.adminCurrentPath(), { timeout: 15_000 }).toMatch(/\/authentication\/$/);

      // Nothing here wrote: the stored values are untouched.
      const kept = await serverAdminGetConfigurations(session);
      expect(kept["GITHUB_CLIENT_ID"]).toBe("parity-keep-id");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-035"], "save gates on dirtiness, shows busy, and resets to the server values"),
  { tag: specTags(["ADM-035"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GOOGLE_CLIENT_ID: "",
        GOOGLE_CLIENT_SECRET: "",
        ENABLE_GOOGLE_SYNC: "0",
      });
      await openProvider(driver, seed, "google");
      expect((await driver.adminAuthFormFacts()).saveDisabled).toBe(true);

      // An unfinished required field blocks the submit silently: no
      // request, no notice, and the form stays dirty.
      await driver.adminStartConfigPatchSpy();
      await driver.adminAuthFormFill({ GOOGLE_CLIENT_ID: "parity-gating-id" });
      expect((await driver.adminAuthFormFacts()).saveDisabled).toBe(false);
      await driver.adminAuthFormSave();
      await sleep(2000);
      expect(await driver.adminConfigPatchBodies()).toEqual([]);
      expect(await driver.adminLastToast()).toBeNull();
      expect((await driver.adminAuthFormFacts()).saveDisabled).toBe(false);

      // A valid save shows the busy label, then resets clean.
      await driver.adminAuthFormFill({ GOOGLE_CLIENT_SECRET: "parity-gating-secret" });
      await driver.adminDelayConfigPatch(3000);
      try {
        await driver.adminAuthFormSave();
        await expect
          .poll(async () => (await driver.adminAuthFormFacts()).saveLabel, { timeout: 15_000 })
          .toBe("Saving");
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        await expect
          .poll(async () => driver.adminLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Done!",
            message: "Your Google authentication is configured. You should test it now.",
          });
        const form = await driver.adminAuthFormFacts();
        expect(form.saveLabel).toBe("Save changes");
        expect(form.saveDisabled).toBe(true);
      } finally {
        await driver.adminClearConfigPatchFault();
      }
      const saved = await serverAdminGetConfigurations(session);
      expect(saved["GOOGLE_CLIENT_ID"]).toBe("parity-gating-id");
      expect(saved["GOOGLE_CLIENT_SECRET"]).toBe("parity-gating-secret");
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-035"], "failed provider save stays silent apart from a console error"),
  { tag: specTags(["ADM-035"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await serverAdminUpdateConfigurations(session, {
        GOOGLE_CLIENT_ID: "parity-kept-id",
        GOOGLE_CLIENT_SECRET: "parity-kept-secret",
        ENABLE_GOOGLE_SYNC: "0",
      });
      await openProvider(driver, seed, "google");
      await driver.adminAuthFormFill({
        GOOGLE_CLIENT_ID: "parity-failure-id",
        GOOGLE_CLIENT_SECRET: "parity-failure-secret",
      });
      await driver.adminStartConfigPatchSpy();
      await driver.adminStartConsoleSpy();
      await driver.adminFailConfigPatch();
      try {
        await driver.adminAuthFormSave();
        await expect.poll(async () => (await driver.adminConfigPatchBodies()).length, { timeout: 15_000 }).toBe(1);
        await expect
          .poll(
            async () =>
              (await driver.adminConsoleErrors()).some((line) =>
                line.includes("Error updating the instance configurations")
              ),
            { timeout: 15_000 }
          )
          .toBe(true);
        await expect
          .poll(async () => (await driver.adminAuthFormFacts()).saveLabel, { timeout: 15_000 })
          .toBe("Save changes");
        // No notice, the form stays dirty, and the server is untouched.
        await sleep(2000);
        expect(await driver.adminLastToast()).toBeNull();
        expect((await driver.adminAuthFormFacts()).saveDisabled).toBe(false);
        const kept = await serverAdminGetConfigurations(session);
        expect(kept["GOOGLE_CLIENT_ID"]).toBe("parity-kept-id");
        expect(kept["GOOGLE_CLIENT_SECRET"]).toBe("parity-kept-secret");
      } finally {
        await driver.adminClearConfigPatchFault();
      }
    } finally {
      await restoreAuth(session, before);
    }
  }
);

test(
  specTitle(["ADM-036"], "landing shows a multi-bar skeleton until config loads"),
  { tag: specTags(["ADM-036"]) },
  async ({ driver, seed }: { driver: ParityDriver; seed: ParitySeedFacts }) => {
    const session = await serverAdminSession(seed.email, seed.password);
    const before = await snapshotAuth(session);
    try {
      await driver.adminEnsureSignedIn(seed.email, seed.password);
      // Cold compile can outlast a short hold on the new console.
      await driver.adminDelayConfigFetch(driver.target === "web_new" ? 120_000 : 4000);
      try {
        await driver.adminOpenPath("/authentication/");
        await expect
          .poll(
            async () => {
              const facts = await driver.adminAuthLandingFacts();
              return facts.skeletonVisible && facts.skeletonBars === 5 && !facts.loaded && facts.cards.length === 0
                ? "skeleton-only"
                : "waiting";
            },
            { timeout: 15_000 }
          )
          .toBe("skeleton-only");
      } finally {
        await driver.adminClearConfigFetchFault();
      }
      await expect.poll(async () => (await driver.adminAuthLandingFacts()).loaded, { timeout: 15_000 }).toBe(true);
      const facts = await driver.adminAuthLandingFacts();
      expect(facts.cards).toHaveLength(6);
      expect(facts.skeletonVisible).toBe(false);
    } finally {
      await restoreAuth(session, before);
    }
  }
);
