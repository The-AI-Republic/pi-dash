// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-124): the project create flow. Rows: SHELL-040
// (identity + auto-derived short code + targeted duplicate errors), SHELL-041
// (defaults: public visibility, work-items-only features), SHELL-042 (no
// cover step on the creation path, so no cover-failure mode).
// Target alignment (NEWFRONT-295 gate, main#523): the form offers no
// cover control, the create POST carries no cover value, and a broken
// asset backend cannot produce a cover warning. No oracle: the old app
// still prefills and uploads covers.
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
      expect(await driver.createFormCoverVisible()).toBe(false);
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
      // Gate note (NEWFRONT-295): web_new renders duplicate errors as
      // persistent inline field alerts rather than expiring toasts, so
      // that leg has no notice to wait out — the next submit replaces
      // the alert in place.
      if (driver.target !== "web_new") {
        await expect.poll(() => driver.isToastVisible("name is already taken")).toBe(false);
      }
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

    await test.step("the fresh form shows no cover and a prefilled icon", async () => {
      expect(await driver.createFormCoverVisible()).toBe(false);
      expect(await driver.createFormIconVisible()).toBe(true);
    });

    // Capture the create POST body: the target behavior sends no cover
    // value at all. The pattern matches only the single-segment create
    // URL (bulk POSTs carry extra segments).
    let createBody: Record<string, unknown> | null = null;
    await driver.page.route(`**/api/workspaces/${ws.slug}/projects/`, async (route) => {
      const request = route.request();
      if (request.method() === "POST") {
        try {
          createBody = JSON.parse(request.postData() ?? "{}") as Record<string, unknown>;
        } catch {
          createBody = {};
        }
      }
      await route.continue();
    });

    await driver.fillCreateProjectName("Defaults Demo");
    await driver.submitCreateProject();

    await test.step("the create POST carries no cover value", async () => {
      await expect.poll(() => projectByName(owner, ws.slug, "Defaults Demo")).not.toBeUndefined();
      expect(createBody).not.toBeNull();
      expect(Object.keys(createBody ?? {}).filter((key) => key.toLowerCase().includes("cover"))).toEqual([]);
    });

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
  specTitle(COVER, "creation submits with no cover step and no asset-backend dependency"),
  { tag: specTags(COVER) },
  async ({ driver }) => {
    // Neutral workspace name: the no-warning assertion scans visible
    // page text, and the workspace name renders in the shell.
    const { owner, ws } = await freshOwnerWorkspace("asset");
    // A prior project keeps the header create button available and proves the
    // empty state is not what we are exercising here. (Named cover-free:
    // the no-warning assertion below scans visible page text.)
    await createProjectViaApi(owner, ws.slug, { name: "Plain Anchor", identifier: "PLAN" });

    // Break the asset service outright: with no cover step on the
    // creation path, creation must not touch it and no cover warning
    // can appear. The pattern matches only the single-segment grant
    // URL (status PATCH and bulk POSTs carry extra segments), and the
    // POST guard keeps boot-time GETs untouched.
    await driver.page.route("**/api/assets/v2/workspaces/*/", async (route) => {
      if (route.request().method() === "POST") {
        await route.abort();
      } else {
        await route.continue();
      }
    });

    await driver.openAuthenticated(`/${ws.slug}/projects`, browserSessionCookies(owner));
    await driver.awaitProjectCard("Plain Anchor");
    await driver.clickHeaderCreateButton();
    await driver.fillCreateProjectName("Simple Creation");
    await driver.submitCreateProject();

    await test.step("the project is created with no cover warning and no cover rendered", async () => {
      await expect.poll(() => projectByName(owner, ws.slug, "Simple Creation")).not.toBeUndefined();
      await driver.awaitProjectCard("Simple Creation");
      expect(await driver.createProjectErrorText()).toBeNull();
      expect(await driver.isToastVisible("cover")).toBe(false);
      expect(await driver.cardHasCoverImage("Simple Creation")).toBe(false);
    });
  }
);
