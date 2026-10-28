// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): epics create through the relations
// widget. Row: ISS-232 (create half). Edition-gated proof per
// parity/specs/issues.md §23: the OSS leg lives in edition-stubs.spec.ts,
// this file proves the cloud replacement. It runs on the "new" project
// only — the OSS oracle provably lacks the surface. Auth rides API
// session cookies (the cloud /login is OIDC-only). The edit half waits
// on the read-side grant (NEWFRONT-1 process note): the relation editor
// routes to the epic dialog only when the detail read carries is_epic,
// which no live read sends yet.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateIssueType,
  serverCreateProjectWithFlags,
  serverEpics,
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
