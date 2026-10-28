// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud scenarios (NEWFRONT-984): the workflow transition gate. Row:
// ISS-233 (gate mechanism only). Edition-gated proof per
// parity/specs/issues.md §23: the OSS leg lives in edition-stubs.spec.ts,
// this file proves the cloud replacement. It runs on the "new" project
// only — the OSS oracle provably lacks the surface. Auth rides API
// session cookies (the cloud /login is OIDC-only).
//
// Provisional standing: no rule source exists yet (no workflow model,
// endpoint, or settings surface in either repo), so the cloud bootstrap
// registers the development-only injected rules
// (features/issues/edition/workflowParityRules.ts) and this leg injects
// one rule through the documented session key before load. It proves the
// gate mechanism — the refused option greys with its reason, the refused
// move never writes, an unruled move still lands — not a configuration
// surface. NEWFRONT-994 replaces the provider and rewires this leg;
// until then ISS-233 stays out of new-green.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCreateIssueFull,
  serverCreateProjectWithFlags,
  serverCreateState,
  serverDefaultStateId,
  serverIssueRecord,
  signInSession,
} from "../helpers/api";
import { browserCookiesFromHeader } from "../helpers/cloud-session";
import { specTags, specTitle } from "../helpers/tags";

// The documented seam contract (workflowParityRules.ts). The leg fails
// loudly on drift: without the key no option greys.
const PARITY_WORKFLOW_RULES_KEY = "pidash.parity.workflowRules.v1";

test(
  specTitle(["ISS-233"], "cloud: the gate refuses the ruled move and allows the rest"),
  { tag: specTags(["ISS-233"]) },
  async ({ driver, seed, page, context }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "cloud workflow pins the web_new UI only");
    const tag = `NF984 workflow ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N984"),
      {},
      session
    );
    const todoId = await serverDefaultStateId(seed.workspaceSlug, projectId, session);
    const reviewName = `${tag} review`;
    const doneName = `${tag} done`;
    const reviewId = await serverCreateState(seed.workspaceSlug, projectId, reviewName, "started", session);
    const doneId = await serverCreateState(seed.workspaceSlug, projectId, doneName, "completed", session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    const reason = "Finish review first";
    try {
      await context.addCookies(browserCookiesFromHeader(session));
      await context.addInitScript(
        ({ key, json }: { key: string; json: string }) => {
          window.sessionStorage.setItem(key, json);
        },
        { key: PARITY_WORKFLOW_RULES_KEY, json: JSON.stringify([{ from: todoId, to: reviewId, reason }]) }
      );
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      const stored = await serverIssueRecord(seed.workspaceSlug, projectId, issue.id, session);
      expect(stored["state_id"]).toBe(todoId);

      await test.step("the ruled option greys with its reason", async () => {
        await driver.propertyOpenPicker("State");
        await expect.poll(() => driver.pickerOptionDisabled(reviewName), { timeout: 60_000 }).toBe(true);
        // The state picker paints the reason as the option's second line
        // (not the trailing span the generic reason reader splits), so the
        // leg reads it off the refused option itself.
        const refused = page.getByRole("listbox").last().getByRole("option", { name: reviewName });
        await expect(refused).toContainText(reason, { timeout: 60_000 });
        await expect.poll(() => driver.pickerOptionDisabled(doneName), { timeout: 60_000 }).toBe(false);
        await driver.pickerClickOutside();
      });

      await test.step("the refused move never writes", async () => {
        const after = await serverIssueRecord(seed.workspaceSlug, projectId, issue.id, session);
        expect(after["state_id"]).toBe(todoId);
      });

      await test.step("an unruled move still lands", async () => {
        await driver.propertyOpenPicker("State");
        await driver.pickerPick(doneName);
        await expect.poll(() => driver.propertyValueText("State"), { timeout: 60_000 }).toContain(doneName);
        await expect
          .poll(async () => (await serverIssueRecord(seed.workspaceSlug, projectId, issue.id, session))["state_id"], {
            timeout: 30_000,
          })
          .toBe(doneId);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session).catch(() => {});
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
