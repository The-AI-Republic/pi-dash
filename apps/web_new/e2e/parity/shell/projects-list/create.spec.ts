// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
  markOnboardedForProjects,
  projectByName,
  projectFeatureFlags,
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
  const ws = await createWorkspaceForProjects(owner, { name: `${prefix} WS ${suffix}`, slug: `${prefix}-${suffix}` });
  await markOnboardedForProjects(owner);
  await setLastWorkspaceForProjects(owner, ws.id);
  return { owner, ws };
}

test(
  specTitle(CREATE, "create captures identity with an auto short code and duplicate errors"),
  { tag: specTags(CREATE) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("create");

    await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(owner));

    await test.step("a fresh name derives a short code and creates the project", async () => {
      await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
      await driver.clickEmptyStateCreate();
      expect(await driver.isCreateProjectDialogVisible()).toBe(true);
      await driver.fillCreateProjectName("Nebula Craft");
      await expect.poll(() => driver.createProjectShortCodeValue()).not.toBe("");
      await driver.submitCreateProject();
      await expect.poll(() => projectByName(owner, ws.slug, "Nebula Craft")).not.toBeUndefined();
    });

    await test.step("a reused name yields a field-specific error", async () => {
      await driver.clickHeaderCreateButton();
      await driver.fillCreateProjectName("Nebula Craft");
      await driver.fillCreateProjectShortCode("NEBX");
      await driver.submitCreateProject();
      await expect.poll(() => driver.createProjectErrorText()).toContain("name is already taken");
    });
  }
);

test(
  specTitle(DEFAULTS, "new projects default to public visibility and work-items-only features"),
  { tag: specTags(DEFAULTS) },
  async ({ driver }) => {
    const { owner, ws } = await freshOwnerWorkspace("defaults");

    await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await expect.poll(() => driver.emptyStateHeading()).toBe("No active projects");
    await driver.clickEmptyStateCreate();
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

    // Make the asset backend unreachable so the cover upload fails.
    await driver.page.route("**/*asset*/**", async (route) => {
      await route.abort();
    });

    await driver.openAuthenticatedAt(`/${ws.slug}/projects`, browserSessionCookies(owner));
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
