// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-428): cross-cutting rows over the profile
// assistant, upkeep, and integrations tabs — no shortcuts, drag targets,
// imports, or live updates (PROF-027), and settings edits touching only
// the sign-in user's own record with role changing nothing (PROF-028).
// Rows PROF-027, PROF-028.
//
// Observed behavior notes (the inventory rows carry them at update time):
// key presses land nowhere (no bindings, no dialogs, no navigation);
// tabs never refetch on their own and open no sockets — server-side
// changes stay invisible until navigation; no tab carries draggable
// elements, file inputs, or import/export mentions; admin and member see
// identical assistant/upkeep controls for identical state, edits land
// only on the actor's own record, and no tab offers a member picker
// (the integrations workspace list stays admin-scoped per PROF-025).
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  markOnboardedForProjects,
  ROLE,
  seatFreshMember,
  serverAssistantConfigDelete,
  serverAssistantConfigGet,
  serverAssistantConfigPut,
  serverAssistantMcpCreate,
  serverAssistantMcpDelete,
  serverLoopJobSetEnabled,
  signInAuthedSession,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

/** The seeded builtin upkeep job (kept disabled for the empty-state halves). */
const JOB_SLUG = "auto-close-merged";

const TABS = ["ai-assistant", "auto-project-management", "integrations"] as const;

/** Seat a fresh workspace member, onboarded and ready to browse settings. */
async function freshSettingsUser(
  admin: AuthedSession,
  seed: ParitySeedFacts,
  prefix: string
): Promise<{ user: AuthedSession; cookies: ReturnType<typeof browserSessionCookies> }> {
  const user = await seatFreshMember(admin, seed.workspaceSlug, ROLE.MEMBER, prefix);
  await markOnboardedForProjects(user);
  return { user, cookies: browserSessionCookies(user) };
}

test(
  specTitle(["PROF-027"], "key presses do nothing on these tabs"),
  { tag: specTags(["PROF-027"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { cookies } = await freshSettingsUser(admin, seed, "parity-prof27k");

    for (const tab of TABS) {
      await test.step(`${tab}: presses change nothing`, async () => {
        await driver.settingsProfileOpenTab(tab, cookies);
        const path = await driver.settingsProfileCurrentPath();
        await driver.settingsProfilePressKeys(["?", "/", "g", "d", "e", "Enter", "Escape", "ArrowDown"]);
        expect(await driver.settingsProfileCurrentPath()).toBe(path);
        expect(await driver.settingsProfileDialogOpen()).toBe(false);
        expect(await driver.settingsProfileLastToast()).toBeNull();
      });
    }
  }
);

test(
  specTitle(["PROF-027"], "nothing refreshes on its own; lists update on navigation"),
  { tag: specTags(["PROF-027"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { user, cookies } = await freshSettingsUser(admin, seed, "parity-prof27l");

    await test.step("an idle tab fires no reads and opens no sockets", async () => {
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      await driver.settingsProfileWatchReads();
      await driver.settingsProfileWatchSockets();
      await driver.page.waitForTimeout(6000);
      expect(await driver.settingsProfileReadCount()).toBe(0);
      expect(await driver.settingsProfileSocketCount()).toBe(0);
    });

    await test.step("server-side changes stay invisible until navigation", async () => {
      await serverAssistantConfigPut(user.cookie, {
        provider_kind: "openai_compatible",
        base_url: "http://127.0.0.1:9",
        model_name: "live-proof-model",
        api_key: "parity-dummy-key-27",
      });
      await serverAssistantMcpCreate(user.cookie, { name: "live-proof-tools", url: "https://mcp.example.com/x" });
      await driver.page.waitForTimeout(2000);
      expect(await driver.settingsProfileReadCount()).toBe(0);
      expect((await driver.settingsProfileAssistantFacts()).model).toBe("");
      expect(await driver.settingsProfileToolServerRows()).toEqual([]);
      await driver.settingsProfileOpenTab("ai-assistant", cookies);
      // Values fill from the data read after paint; poll until they land.
      await expect
        .poll(async () => (await driver.settingsProfileAssistantFacts()).model, { timeout: 15_000 })
        .toBe("live-proof-model");
      await expect
        .poll(async () => (await driver.settingsProfileToolServerRows()).map((row) => row.name), { timeout: 15_000 })
        .toEqual(["live-proof-tools"]);
    });

    for (const tab of ["auto-project-management", "integrations"] as const) {
      await test.step(`${tab}: no sockets while idle`, async () => {
        await driver.settingsProfileOpenTab(tab, cookies);
        await driver.settingsProfileWatchSockets();
        await driver.page.waitForTimeout(3000);
        expect(await driver.settingsProfileSocketCount()).toBe(0);
      });
    }
  }
);

test(
  specTitle(["PROF-027"], "no drag targets or import/export paths on these tabs"),
  { tag: specTags(["PROF-027"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const { cookies } = await freshSettingsUser(admin, seed, "parity-prof27d");

    for (const tab of TABS) {
      await test.step(`${tab}: nothing draggable, nothing to import or export`, async () => {
        await driver.settingsProfileOpenTab(tab, cookies);
        expect(await driver.settingsProfileDraggableCount()).toBe(0);
        expect(await driver.settingsProfileFileInputCount()).toBe(0);
        expect(await driver.settingsProfileImportExportMentions()).toEqual([]);
      });
    }
  }
);

test(
  specTitle(["PROF-028"], "settings touch only the sign-in user; role changes nothing"),
  { tag: specTags(["PROF-028"]) },
  async ({ driver, seed }) => {
    const admin = await signInAuthedSession(seed.email, seed.password);
    const adminCookies = browserSessionCookies(admin);
    await serverLoopJobSetEnabled(JOB_SLUG, false);
    const { user: member, cookies: memberCookies } = await freshSettingsUser(admin, seed, "parity-prof28");

    // Identical server state for both roles, so any control difference is
    // role-caused. The seed admin's rows are removed afterwards.
    const seededConfig = {
      provider_kind: "openai_compatible",
      base_url: "http://127.0.0.1:9",
      model_name: "role-proof-model",
      api_key: "parity-dummy-key-28",
    };
    await serverAssistantConfigPut(admin.cookie, seededConfig);
    await serverAssistantConfigPut(member.cookie, seededConfig);
    const adminMcp = await serverAssistantMcpCreate(admin.cookie, {
      name: "role-proof-tools",
      url: "https://mcp.example.com/r",
    });
    const memberMcp = await serverAssistantMcpCreate(member.cookie, {
      name: "role-proof-tools",
      url: "https://mcp.example.com/r",
    });
    try {
      await test.step("assistant controls are identical across roles", async () => {
        await driver.settingsProfileOpenTab("ai-assistant", adminCookies);
        // Values fill from the data read after paint; settle each side on
        // the seeded values before comparing the full control shapes.
        await expect
          .poll(async () => (await driver.settingsProfileAssistantFacts()).model, { timeout: 15_000 })
          .toBe("role-proof-model");
        await expect
          .poll(async () => (await driver.settingsProfileToolServerRows()).map((row) => row.name), { timeout: 15_000 })
          .toEqual(["role-proof-tools"]);
        const adminFacts = await driver.settingsProfileAssistantFacts();
        const adminDictation = await driver.settingsProfileDictationFacts();
        const adminRows = await driver.settingsProfileToolServerRows();
        await driver.settingsProfileOpenTab("ai-assistant", memberCookies);
        await expect.poll(async () => driver.settingsProfileAssistantFacts(), { timeout: 15_000 }).toEqual(adminFacts);
        expect(await driver.settingsProfileDictationFacts()).toEqual(adminDictation);
        expect(await driver.settingsProfileToolServerRows()).toEqual(adminRows);
      });

      await test.step("upkeep shows both roles the same empty explanation", async () => {
        await driver.settingsProfileOpenTab("auto-project-management", adminCookies);
        const adminFacts = await driver.settingsProfileUpkeepFacts();
        await driver.settingsProfileOpenTab("auto-project-management", memberCookies);
        expect(await driver.settingsProfileUpkeepFacts()).toEqual(adminFacts);
        expect(adminFacts.emptyExplanation).toBe("Nothing is scheduled on this instance yet.");
      });

      await test.step("a member edit lands only on the member's own record", async () => {
        const before = await serverAssistantConfigGet(admin.cookie);
        await driver.settingsProfileOpenTab("ai-assistant", memberCookies);
        // Settle on the seeded values first: filling before the
        // server-population effect lands would be overwritten by it.
        await expect
          .poll(async () => (await driver.settingsProfileAssistantFacts()).model, { timeout: 15_000 })
          .toBe("role-proof-model");
        await driver.settingsProfileAssistantFill("model", "member-edited-model");
        await driver.settingsProfileAssistantSave();
        await expect
          .poll(() => driver.settingsProfileLastToast(), { timeout: 15_000 })
          .toEqual({
            title: "Saved",
            message: "AI provider configuration updated.",
          });
        expect((await serverAssistantConfigGet(member.cookie)).model_name).toBe("member-edited-model");
        expect(await serverAssistantConfigGet(admin.cookie)).toEqual(before);
      });

      for (const tab of TABS) {
        await test.step(`${tab}: no member picker for either role`, async () => {
          await driver.settingsProfileOpenTab(tab, adminCookies);
          expect(await driver.settingsProfileMemberControlCount()).toBe(0);
          await driver.settingsProfileOpenTab(tab, memberCookies);
          expect(await driver.settingsProfileMemberControlCount()).toBe(0);
        });
      }

      await test.step("members keep the no-admin integrations view", async () => {
        await driver.settingsProfileOpenTab("integrations", memberCookies);
        expect((await driver.settingsProfileIntegrationsFacts()).noAdmin).toBe(true);
      });
    } finally {
      await serverAssistantConfigDelete(admin.cookie).catch(() => undefined);
      await serverAssistantMcpDelete(adminMcp.id, admin.cookie).catch(() => undefined);
      await serverAssistantConfigDelete(member.cookie).catch(() => undefined);
      await serverAssistantMcpDelete(memberMcp.id, member.cookie).catch(() => undefined);
    }
  }
);
