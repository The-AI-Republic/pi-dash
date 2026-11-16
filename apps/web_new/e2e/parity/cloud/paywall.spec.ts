// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): free-cap paywall, row CLOUD-010
// (hitting the free creation cap surfaces a paywall prompt), green on
// the cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud marketing area lands there.
//
// Observed behavior notes (cloud oracle): the dev-login session carries
// the Free plan (500 created issues); the creation endpoint refuses the
// 501st with a 402 quota payload naming the limit and an upgrade URL;
// the app answers with a single native confirm prompt, and accepting it
// opens the upgrade destination in a new tab — one prompt per refused
// attempt, never stacked. The scenario fills the cap through the API in
// a fixed per-stack project (convergent: a retry adopts the project and
// breaks out of the fill on the first 402), then drives the refused
// creation through the create modal.
import { test, expect } from "../fixtures";
import {
  cloudBrowserCookies,
  cloudCreateIssueRaw,
  cloudDevLogin,
  createProjectViaApi,
  serverProjects,
  uniqueSuffix,
  type AuthedSession,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";

const FREE_CAP = 500;
// Fixed per-stack project: project creation is quota-limited too, so a
// retry (already capped) adopts the project the first attempt created
// instead of creating another.
const QUOTA_PROJECT_IDENTIFIER = "QPW";

function asAuthed(email: string, userId: string, cookie: string): AuthedSession {
  return { email, password: "", userId, cookie, csrfToken: "", apiBase: parityApiUrl(process.env) };
}

/** Adopt the quota project, creating it on the first attempt. */
async function quotaProjectId(authed: AuthedSession, workspaceSlug: string): Promise<string> {
  const existing = await serverProjects(workspaceSlug, authed.cookie, authed.apiBase);
  const found = existing.find((row) => row.identifier === QUOTA_PROJECT_IDENTIFIER);
  if (found !== undefined) return found.id;
  return createProjectViaApi(authed, workspaceSlug, {
    name: "Quota proving ground",
    identifier: QUOTA_PROJECT_IDENTIFIER,
  });
}

test(
  specTitle(["CLOUD-010"], "refused creation prompts once and opens the upgrade target"),
  { tag: specTags(["CLOUD-010"]) },
  async ({ driver, seed }) => {
    const session = await cloudDevLogin(seed.email);
    const authed = asAuthed(seed.email, session.userId, session.cookie);
    const projectId = await quotaProjectId(authed, seed.workspaceSlug);

    // Fill to the cap through the API; an already-capped account (a
    // retry of this scenario) breaks out on the first refusal.
    let refusal: Record<string, unknown> | null = null;
    for (let round = 0; round <= FREE_CAP + 5; round++) {
      const answer = await cloudCreateIssueRaw(
        seed.workspaceSlug,
        projectId,
        session.cookie,
        `cap fill ${round} ${uniqueSuffix()}`
      );
      if (answer.status === 201) continue;
      expect(answer.status).toBe(402);
      refusal = answer.payload;
      break;
    }
    expect(refusal).not.toBeNull();
    expect(refusal?.["error"]).toBe("quota_exceeded");
    expect(refusal?.["code"]).toBe("free_issue_creation_limit_exceeded");
    expect(refusal?.["limit"]).toBe(FREE_CAP);
    expect(refusal?.["used"]).toBe(FREE_CAP);
    const upgradeUrl = refusal?.["upgrade_url"];
    expect(typeof upgradeUrl).toBe("string");
    expect((upgradeUrl as string).length).toBeGreaterThan(0);

    // The same refusal through the UI: one confirm naming the limit, and
    // accepting it opens the upgrade destination from the payload.
    await driver.openAuthenticated(
      `/${seed.workspaceSlug}/projects/${projectId}/issues`,
      cloudBrowserCookies(session.cookie)
    );
    await driver.dismissWelcomeDialog();
    await driver.openCreateModal();
    await driver.fillCreateTitle("Paywalled work item");
    await driver.cloudArmConfirm("accept");
    await driver.submitCreateModal();
    await expect.poll(async () => (await driver.cloudConfirmMessages()).length, { timeout: 60_000 }).toBe(1);
    const first = await driver.cloudConfirmMessages();
    // The prompt names the limit qualitatively (the numeric cap travels in
    // the 402 payload, asserted above).
    expect(first[0]?.toLowerCase()).toContain("limit");
    expect(await driver.cloudUpgradePopupUrl()).toBe(upgradeUrl);

    // A second refused attempt prompts exactly once more — prompts never stack.
    if (!(await driver.createModalOpen())) await driver.openCreateModal();
    await driver.fillCreateTitle("Paywalled work item again");
    await driver.cloudArmConfirm("dismiss");
    await driver.submitCreateModal();
    await expect.poll(async () => (await driver.cloudConfirmMessages()).length, { timeout: 60_000 }).toBe(1);

    // The refused attempts stored nothing: the cap still reads exactly full.
    const after = await cloudCreateIssueRaw(seed.workspaceSlug, projectId, session.cookie, "cap probe");
    expect(after.status).toBe(402);
    expect(after.payload["used"]).toBe(FREE_CAP);
  }
);
