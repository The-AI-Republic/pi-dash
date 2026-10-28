// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): epics create and edit through the
// relations widget. Row: ISS-232. Edition-gated proof per
// parity/specs/issues.md §23: the OSS leg lives in edition-stubs.spec.ts,
// this file proves the cloud replacement. It runs on the "new" project
// only — the OSS oracle provably lacks the surface. Auth rides API
// session cookies (the cloud /login is OIDC-only).
import { test, expect } from "../fixtures";
import {
  addRelation,
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateIssueType,
  serverCreateProjectWithFlags,
  serverEpics,
  serverPatchIssue,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-232"], "cloud: epics create through the relations widget"),
  { tag: specTags(["ISS-232"]) },
  async ({ driver, seed, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud epics pin the web_new UI only");
    const tag = `NF984 epic ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    await serverCreateIssueType(seed.workspaceSlug, projectId, "Epic", session, { isEpic: true });
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const epicName = `${tag} epic`;
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the widget action creates the epic", async () => {
        await driver.epicCreateViaWidget(epicName);
        await expect
          .poll(async () => (await serverEpics(seed.workspaceSlug, projectId, session)).map((row) => row.name), {
            timeout: 30_000,
          })
          .toContain(epicName);
      });
    } finally {
      const epics = await serverEpics(seed.workspaceSlug, projectId, session).catch(() => []);
      for (const epic of epics.filter((row) => row.name.startsWith(tag))) {
        await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, epic.id, session).catch(() => {});
      }
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-232"], "cloud: epics edit through the relation row"),
  { tag: specTags(["ISS-232"]) },
  async ({ driver, seed, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud epics pin the web_new UI only");
    const tag = `NF984 epicedit ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const epicType = await serverCreateIssueType(seed.workspaceSlug, projectId, "Epic", session, { isEpic: true });
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const epic = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} epic`, session);
    await serverPatchIssue(seed.workspaceSlug, projectId, epic.id, { type: epicType.id }, session);
    await addRelation(seed.workspaceSlug, projectId, issue.id, session, {
      relation_type: "relates_to",
      issues: [epic.id],
    });
    const renamed = `${tag} renamed`;
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("the relation row edits through the epic dialog", async () => {
        await driver.epicRenameRelationRow(`${tag} epic`, renamed);
        await expect
          .poll(async () => (await serverEpics(seed.workspaceSlug, projectId, session)).map((row) => row.name), {
            timeout: 30_000,
          })
          .toContain(renamed);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, epic.id, session).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
