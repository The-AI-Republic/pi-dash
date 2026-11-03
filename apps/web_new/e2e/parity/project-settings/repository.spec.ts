// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-389): the repository bind control on the
// General tab — its own admin-only control outside the normal save
// payload, rejecting empty input; backend failures never reach the user
// (bug NEWFRONT-395). Rows: PSET-008.
//
// Observed behavior notes: typing in the repository row never dirties the
// submit; only Bind persists, through the bind endpoint. The seeded stack
// has no git provider connected, so every bind attempt answers 401
// {"error":"GitHub token is missing"} — and that 401 trips the global
// session interceptor, which reloads the page to / and back onto the tab
// via next_path, so no "Bind failed" toast ever renders. The bug: scenario
// pins that silence with no state change. The success clauses (bound pill
// on match, canonical URL written back, sync pointer) need a connected
// provider and are recorded in the area spec as unproven on the seeded
// stack; they must gain a scenario wherever a provider-backed stack
// exists.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverBindRepositoryRaw,
  serverCleanupProject,
  serverCreateProjectWithFlags,
  serverProjectRecord,
  serverRepositoryStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["PSET-008"], "repository row stays out of the normal save and rejects empty binds"),
  { tag: specTags(["PSET-008"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 repo ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await test.step("a fresh project offers Bind with no binding server-side", async () => {
        expect(await driver.settingsGeneralBindState()).toBe("bind");
        expect(String((await serverRepositoryStatus(seed.workspaceSlug, projectId, session))["bound"])).toBe("false");
      });

      await test.step("empty input is refused with an error notice", async () => {
        await driver.settingsGeneralFill("repoUrl", "");
        await driver.settingsGeneralBind();
        const toast = await driver.rulesLastToast();
        expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`).toContain("Enter a Git repository URL first.");
      });

      await test.step("typing a URL never dirties the normal save payload", async () => {
        await driver.settingsGeneralFill("repoUrl", "https://github.com/octocat/Hello-World");
        await driver.settingsGeneralFill("description", `${tag} touch`);
        await driver.settingsGeneralSave();
        expect(String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["repo_url"] ?? "")).toBe("");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-008"], "bug: the provider-missing bind refusal reloads the tab instead of surfacing (NEWFRONT-395)"),
  { tag: specTags(["PSET-008"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 bindfail ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      // The seeded stack has no provider: the backend refuses every bind
      // with its provider-missing error. Intended: the UI toasts "Bind
      // failed" with that message verbatim. Actual: the 401 trips the
      // global session interceptor, which reloads to / and back onto the
      // tab — no toast ever renders. If fixed this fails: drop the bug:
      // marker and assert the toast instead.
      const refusal = await serverBindRepositoryRaw(
        seed.workspaceSlug,
        projectId,
        "https://github.com/octocat/Hello-World",
        session
      );
      expect(refusal.status).toBe(401);
      expect(refusal.body).toContain("GitHub token is missing");

      await driver.settingsGeneralFill("repoUrl", "https://github.com/octocat/Hello-World");
      await driver.settingsGeneralBind();
      expect(await driver.settingsGeneralFormVisible()).toBe(true);
      expect(await driver.settingsGeneralBindState()).toBe("bind");
      expect(await driver.rulesLastToast()).toBeNull();
      expect(String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["repo_url"] ?? "")).toBe("");
      expect(String((await serverRepositoryStatus(seed.workspaceSlug, projectId, session))["bound"])).toBe("false");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
