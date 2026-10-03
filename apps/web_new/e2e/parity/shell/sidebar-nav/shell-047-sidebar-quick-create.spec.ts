// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenario (NEWFRONT-125): the sidebar quick-create starts a work
// item, and stays disabled with nowhere to put it or when the role
// forbids it.
// Observed on the running old app: workspace admins and members see an
// enabled New work item control that opens the creation dialog scoped to
// their workspace; guests, and members who joined no project, see the
// control disabled. Row: SHELL-047.
import { test, expect } from "../../fixtures";
import { WORKSPACE_ROLE_GUEST, WORKSPACE_ROLE_MEMBER, ensureWorkspaceMember, ownerSession } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-047"];
const GUEST_EMAIL = "parity-sidebar-guest@example.com";
const GUEST_PASSWORD = "Parity-Guest-1";
const PROJECTLESS_EMAIL = "parity-sidebar-noproject@example.com";
const PROJECTLESS_PASSWORD = "Parity-NoProject-1";

test(
  specTitle(ROWS, "sidebar quick-create opens the dialog for members, disabled otherwise"),
  {
    tag: specTags(ROWS),
  },
  async ({ driver, seed }) => {
    const session = await test.step("prepare server session", async () => ownerSession(seed));
    await test.step("provision role variants", async () => {
      await ensureWorkspaceMember(seed.workspaceSlug, session, GUEST_EMAIL, GUEST_PASSWORD, WORKSPACE_ROLE_GUEST);
      await ensureWorkspaceMember(
        seed.workspaceSlug,
        session,
        PROJECTLESS_EMAIL,
        PROJECTLESS_PASSWORD,
        WORKSPACE_ROLE_MEMBER
      );
    });

    await test.step("sign in through the UI", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
    });

    await test.step("eligible members get an enabled control", async () => {
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isQuickCreateEnabled()).toBe(true);
    });

    await test.step("the control opens the creation dialog", async () => {
      await driver.openQuickCreate();
      await expect.poll(() => driver.isQuickCreateDialogOpen(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("guests see the control disabled", async () => {
      await driver.resetSession();
      await driver.openEntry();
      await driver.signInWithPassword(GUEST_EMAIL, GUEST_PASSWORD);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isQuickCreateEnabled()).toBe(false);
    });

    await test.step("members with no joined project see the control disabled", async () => {
      await driver.resetSession();
      await driver.openEntry();
      await driver.signInWithPassword(PROJECTLESS_EMAIL, PROJECTLESS_PASSWORD);
      await driver.openWorkspaceHome(seed.workspaceSlug);
      await expect.poll(() => driver.sidebarLinkTexts(), { timeout: 60_000 }).toContain("Home");
      expect(await driver.isQuickCreateEnabled()).toBe(false);
    });
  }
);
