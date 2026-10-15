// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-124): the project create flow. Rows: SHELL-040
// (identity + auto-derived short code + targeted duplicate errors), SHELL-041
// (defaults: public visibility, work-items-only features), SHELL-042
// (cover-upload failure degrades gracefully instead of blocking creation).
// Green on apps/web first.
import { test, expect } from "../../fixtures";
import {
  NETWORK,
  browserSessionCookies,
  createProjectViaApi,
  createWorkspaceForProjects,
  drainWorkspaceSeed,
  markOnboardedForProjects,
  projectByName,
  projectFeatureFlags,
  projectTimezone,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const CREATE = ["SHELL-040"];
const DEFAULTS = ["SHELL-041"];
const COVER = ["SHELL-042"];

async function freshOwnerWorkspace(prefix: string) {
  const owner = await signUpAuthedSession(prefix);
  const suffix = uniqueSuffixForProjects();
  const wsName = `${prefix} WS ${suffix}`;
  const ws = await createWorkspaceForProjects(owner, { name: wsName, slug: `${prefix}-${suffix}` });
  await drainWorkspaceSeed(owner, ws.slug, wsName);
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  return { owner, ws };
}

test(
  specTitle(CREATE, "create captures identity with an auto short code and duplicate errors"),
  { tag: specTags(CREATE) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("create");

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));

    await test.step("a fresh name derives a short code and creates the project", async () => {
      await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
      await driver.clickEmptyStateCreate();
      await expect.poll(() => driver.isCreateProjectDialogVisible()).toBe(true);
      await driver.fillCreateProjectName("Nebula Craft");
      await expect.poll(() => driver.createProjectShortCodeValue()).not.toBe("");
      await driver.submitCreateProject();
      await expect.poll(() => projectByName(owner, ws.slug, "Nebula Craft")).not.toBeUndefined();
    });

    await test.step("creating closes the modal with a success notice", async () => {
      await expect.poll(() => driver.isCreateProjectDialogVisible()).toBe(false);
      await expect.poll(() => driver.isToastVisible("Project created successfully")).toBe(true);
    });

    await test.step("a reused name yields a field-specific error", async () => {
      await driver.clickHeaderCreateButton();
      await driver.fillCreateProjectName("Nebula Craft");
      await driver.fillCreateProjectShortCode("NEBX");
      await driver.submitCreateProject();
      await expect.poll(() => driver.createProjectErrorText()).toContain("name is already taken");
    });

    await test.step("a reused short code yields a field-specific error", async () => {
      // Let the name-duplicate notice clear before submitting: the error
      // reader returns the first visible notice, so while the name notice
      // is still up the identifier notice is only observable in the
      // sub-second window after the name notice expires — a window the
      // poll's backoff spacing (up to 1s) routinely misses on the fast
      // production oracle (NEWFRONT-361). Waiting keeps the same
      // assertion; it only removes the incidental toast overlap.
      await expect.poll(() => driver.isToastVisible("name is already taken")).toBe(false);
      const existing = await projectByName(owner, ws.slug, "Nebula Craft");
      await driver.fillCreateProjectName("Another Craft");
      await driver.fillCreateProjectShortCode(existing!.identifier);
      await driver.submitCreateProject();
      await expect.poll(() => driver.createProjectErrorText()).toContain("identifier is already taken");
    });
  }
);

test(
  specTitle(DEFAULTS, "new projects default to public visibility and work-items-only features"),
  { tag: specTags(DEFAULTS) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("defaults");

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
    await driver.clickEmptyStateCreate();
    await expect.poll(() => driver.isCreateProjectDialogVisible()).toBe(true);

    await test.step("the fresh form shows its prefilled cover and icon", async () => {
      expect(await driver.createFormCoverVisible()).toBe(true);
      expect(await driver.createFormIconVisible()).toBe(true);
    });

    await driver.fillCreateProjectName("Defaults Demo");
    await driver.submitCreateProject();

    await test.step("the created project is public with cycles/modules/views/pages/intake disabled", async () => {
      await expect.poll(() => projectFeatureFlags(owner, ws.slug, "Defaults Demo")).not.toBeUndefined();
      const flags = await projectFeatureFlags(owner, ws.slug, "Defaults Demo");
      expect(flags?.network).toBe(NETWORK.PUBLIC);
      expect(flags?.cycle_view).toBe(false);
      expect(flags?.module_view).toBe(false);
      expect(flags?.issue_views_view).toBe(false);
      expect(flags?.page_view).toBe(false);
      expect(flags?.intake_view).toBe(false);
    });

    await test.step("the created project carries a prefilled timezone", async () => {
      await expect.poll(() => projectTimezone(owner, ws.slug, "Defaults Demo")).not.toBeNull();
    });
  }
);

test(
  specTitle(COVER, "cover-upload failure degrades to a default cover instead of blocking creation"),
  { tag: specTags(COVER) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("cover");
    // A prior project keeps the header create button available and proves the
    // empty state is not what we are exercising here.
    await createProjectViaApi(owner, ws.slug, { name: "Cover Anchor", identifier: "CVAN" });

    // Fail the cover upload at the asset-service grant so the submit takes
    // the degraded-cover path. Aborting the grant POST (rather than the
    // bundled-image fetch) is build-independent: dev serves bundled covers
    // from a path containing `assets/cover-images`, but the production
    // build flattens them to `/assets/<name>-<hash>.jpg` (NEWFRONT-361),
    // so a fetch-leg abort only injects the failure on dev. The pattern
    // matches only the single-segment grant URL (status PATCH and bulk
    // POSTs carry extra segments), and the POST guard keeps boot-time
    // GETs untouched.
    await driver.page.route("**/api/assets/v2/workspaces/*/", async (route) => {
      if (route.request().method() === "POST") {
        await route.abort();
      } else {
        await route.continue();
      }
    });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Cover Anchor");
    await driver.clickHeaderCreateButton();
    await driver.fillCreateProjectName("Degraded Cover");
    await driver.submitCreateProject();

    await test.step("the project is still created on the default cover with a non-blocking warning", async () => {
      await expect.poll(() => projectByName(owner, ws.slug, "Degraded Cover")).not.toBeUndefined();
      await expect.poll(() => driver.createProjectErrorText()).toContain("default cover");
    });
  }
);
