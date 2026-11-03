// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity scenarios (NEWFRONT-308): modules inline edits, menus, peek and
// timeline. Rows MOD-013 (inline dates), MOD-014 (lifecycle statuses),
// MOD-015 (lead display), MOD-016 (favorites), MOD-017 (row quick menu),
// MOD-018 (copy link / open in new tab), MOD-019 (peek panel), MOD-021
// (timeline drag-to-reschedule). Green on apps/web first.
//
// Fixture shape: every scenario mints its own owner, workspace, and
// project (module view on), because display choices persist per project.
// Modules are created through the API in-spec; the seed carries none.
// Layout and ordering travel through the persisted display choice (the
// switcher UIs are MOD-005/MOD-011, owned by sibling issues).
import { test, expect } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  ROLE,
  addProjectMembersViaApi,
  archivesArchiveModule,
  archivesPatchModule,
  browserSessionCookies,
  createWorkspaceForProjects,
  markOnboardedForProjects,
  modulesFetchModule,
  seatFreshMember,
  serverCreateModule,
  serverCreateProjectWithFlags,
  setLastWorkspaceForProjects,
  signUpAuthedSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { openArchivesModulesTab } from "./chunkRecovery";
import { parityNewUrl, parityOracleUrl } from "../helpers/parity-env";

const ROW_013 = ["MOD-013"];
const ROW_014 = ["MOD-014"];
const ROW_015 = ["MOD-015"];
const ROW_016 = ["MOD-016"];
const ROW_017 = ["MOD-017"];
const ROW_018 = ["MOD-018"];
const ROW_019 = ["MOD-019"];
const ROW_021 = ["MOD-021"];

interface ModulesWorld {
  owner: AuthedSession;
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
}

/** Fresh owner, workspace, and module-view project for one scenario. */
async function setupModulesWorld(prefix: string): Promise<ModulesWorld> {
  const owner = await signUpAuthedSession(prefix);
  // The project create 400s intermittently on a loaded scratch stack;
  // one retry with a fresh suffix rides it out.
  for (let attempt = 1; ; attempt++) {
    const suffix = uniqueSuffixForProjects();
    try {
      const ws = await createWorkspaceForProjects(owner, {
        name: `ModRow WS ${suffix}`,
        slug: `modr-${suffix}`,
      });
      await markOnboardedForProjects(owner);
      await setLastWorkspaceForProjects(owner, ws.id);
      const projectId = await serverCreateProjectWithFlags(
        ws.slug,
        "Row Actions",
        `MR${suffix.slice(-8)}`.toUpperCase(),
        { moduleView: true },
        owner.cookie
      );
      return { owner, workspaceSlug: ws.slug, workspaceId: ws.id, projectId };
    } catch (error) {
      if (attempt >= 2) throw error;
      await new Promise((resolve) => setTimeout(resolve, 2000));
    }
  }
}

/** Seat a workspace guest onto the world's project, onboarded for UI entry. */
async function setupGuest(world: ModulesWorld, prefix: string): Promise<AuthedSession> {
  const guest = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.GUEST, prefix);
  await markOnboardedForProjects(guest);
  await setLastWorkspaceForProjects(guest, world.workspaceId);
  await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
    { member_id: guest.userId, role: ROLE.GUEST },
  ]);
  return guest;
}

/** Open the modules list as `session`; resolves once the named row renders. */
async function enterModulesList(
  driver: ParityDriver,
  world: ModulesWorld,
  session: AuthedSession,
  rowName: string
): Promise<void> {
  await driver.modulesOpenList(world.workspaceSlug, world.projectId, browserSessionCookies(session));
  await driver.modulesAwaitRow(rowName);
}

/** Local yyyy-MM-dd for an exact server-date assertion. */
function toPayloadDate(day: Date): string {
  const month = String(day.getMonth() + 1).padStart(2, "0");
  const date = String(day.getDate()).padStart(2, "0");
  return `${day.getFullYear()}-${month}-${date}`;
}

/** Full month caption the range calendar's month dropdown shows. */
function monthCaption(day: Date): string {
  return day.toLocaleString("en-US", { month: "long" });
}

/**
 * Pick a from/to pair through the range calendar. A single click saves both
 * ends to that day, so the calendar reopens from the settled first save
 * before the second click extends the range — back-to-back clicks race two
 * PATCHes and can land on stale selection.
 */
async function pickRange(driver: ParityDriver, reopen: () => Promise<void>, from: Date, to: Date): Promise<void> {
  await driver.modulesCalendarGotoMonth(monthCaption(from), String(from.getFullYear()));
  await driver.modulesCalendarPickDay(from.getDate());
  await driver.modulesAwaitToast("Module updated successfully.");
  // The second save's toast wait matches any visible notice, so drain
  // the first save's toast before the second pick — otherwise it
  // matches stale and the caller reads the first save's record.
  await expect.poll(() => driver.modulesLastToast(), { timeout: 30_000 }).toBe(null);
  await reopen();
  await driver.modulesCalendarGotoMonth(monthCaption(to), String(to.getFullYear()));
  await driver.modulesCalendarPickDay(to.getDate());
  await driver.modulesAwaitToast("Module updated successfully.");
}

/** Poll the row's date label until it shows `text` (the row re-renders async). */
async function expectRowDateText(driver: ParityDriver, name: string, text: string): Promise<void> {
  await expect.poll(() => driver.modulesRowDateText(name), { timeout: 30_000 }).toBe(text);
}

test(
  specTitle(ROW_013, "row date control shows placeholders, saves a picked range, and clears the range"),
  { tag: specTags(ROW_013) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod013");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Dated Module", world.owner.cookie);
    const from = new Date();
    from.setDate(from.getDate() + 10);
    const to = new Date();
    to.setDate(to.getDate() + 17);

    await enterModulesList(driver, world, world.owner, "Dated Module");

    await test.step("empty control shows the start/end placeholders", async () => {
      expect(await driver.modulesRowDateText("Dated Module")).toBe("Start date End date");
    });

    await test.step("picking a range saves it with a success notice", async () => {
      await driver.modulesRowOpenDates("Dated Module");
      expect(await driver.modulesCalendarVisible()).toBe(true);
      await pickRange(driver, () => driver.modulesRowOpenDates("Dated Module"), from, to);
      const saved = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(saved.startDate).toBe(toPayloadDate(from));
      expect(saved.targetDate).toBe(toPayloadDate(to));
      await expect
        .poll(() => driver.modulesRowDateText("Dated Module"), { timeout: 30_000 })
        .not.toContain("Start date");
    });

    await test.step("re-clicking the selected day clears both sides to empty dates", async () => {
      // Oracle behavior: the row/card calendar has no per-side clear — a
      // click re-anchors both ends to that day, and clicking the single
      // selected day again clears the whole range (both sides empty).
      // Drain pickRange's leftover toast first (same race as pickRange's
      // own drain): each awaitToast below must match its own pick's
      // save, or the server read lands on the previous save's record.
      await expect.poll(() => driver.modulesLastToast(), { timeout: 30_000 }).toBe(null);
      await driver.modulesRowOpenDates("Dated Module");
      await driver.modulesCalendarGotoMonth(monthCaption(from), String(from.getFullYear()));
      await driver.modulesCalendarPickDay(from.getDate());
      await driver.modulesAwaitToast("Module updated successfully.");
      await expect.poll(() => driver.modulesLastToast(), { timeout: 30_000 }).toBe(null);
      await driver.modulesRowOpenDates("Dated Module");
      await driver.modulesCalendarGotoMonth(monthCaption(from), String(from.getFullYear()));
      await driver.modulesCalendarPickDay(from.getDate());
      await driver.modulesAwaitToast("Module updated successfully.");
      const saved = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(saved.startDate).toBeNull();
      expect(saved.targetDate).toBeNull();
      await expectRowDateText(driver, "Dated Module", "Start date End date");
    });
  }
);

test(
  specTitle(ROW_013, "board card date control saves a picked range"),
  { tag: specTags(ROW_013) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod013c");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Card Dated", world.owner.cookie);
    const from = new Date();
    from.setDate(from.getDate() + 10);
    const to = new Date();
    to.setDate(to.getDate() + 17);

    await enterModulesList(driver, world, world.owner, "Card Dated");
    await driver.modulesSetLayout("board");

    await test.step("empty card control shows the start/end placeholders", async () => {
      expect(await driver.modulesCardDateText("Card Dated")).toBe("Start date End date");
    });

    await test.step("picking a range on the card saves it", async () => {
      await driver.modulesCardOpenDates("Card Dated");
      expect(await driver.modulesCalendarVisible()).toBe(true);
      await pickRange(driver, () => driver.modulesCardOpenDates("Card Dated"), from, to);
      const saved = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(saved.startDate).toBe(toPayloadDate(from));
      expect(saved.targetDate).toBe(toPayloadDate(to));
    });
  }
);

test(
  specTitle([...ROW_013, ...ROW_014], "date and status controls disable for guests and on archived modules"),
  { tag: specTags([...ROW_013, ...ROW_014]) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod013d");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Locked Module",
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      moduleId,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
    const guest = await setupGuest(world, "parity-mod013g");
    const liveId = await serverCreateModule(world.workspaceSlug, world.projectId, "Live Module", world.owner.cookie);
    void liveId;

    await test.step("a guest sees both controls disabled on a live row", async () => {
      await enterModulesList(driver, world, guest, "Live Module");
      expect(await driver.modulesRowDateDisabled("Live Module")).toBe(true);
      expect(await driver.modulesStatusDisabled("Live Module")).toBe(true);
    });

    await test.step("an archived row disables both controls for an editor", async () => {
      await driver.modulesOpenList(world.workspaceSlug, world.projectId, browserSessionCookies(world.owner));
      await openArchivesModulesTab(driver, world.workspaceSlug, world.projectId);
      await driver.modulesAwaitRow("Locked Module");
      expect(await driver.modulesRowDateDisabled("Locked Module")).toBe(true);
      expect(await driver.modulesStatusDisabled("Locked Module")).toBe(true);
    });
  }
);

test(
  specTitle(ROW_014, "all six lifecycle states render with their colors and a change persists"),
  { tag: specTags(ROW_014) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod014");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "State Module", world.owner.cookie);
    const states: Array<{ label: string; value: string; color: string }> = [
      { label: "Backlog", value: "backlog", color: "#a3a3a2" },
      { label: "Planned", value: "planned", color: "#3f76ff" },
      { label: "In Progress", value: "in-progress", color: "#f39e1f" },
      { label: "Paused", value: "paused", color: "#525252" },
      { label: "Completed", value: "completed", color: "#16a34a" },
      { label: "Cancelled", value: "cancelled", color: "#ef4444" },
    ];

    await enterModulesList(driver, world, world.owner, "State Module");

    await test.step("the pill shows the current state and all six options render", async () => {
      expect(await driver.modulesPillText("State Module")).toBe("Planned");
      await driver.modulesOpenStatus("State Module");
      expect(await driver.modulesStatusOptions()).toEqual(states.map((state) => state.label));
      await driver.modulesPickStatus("Backlog");
      await driver.modulesAwaitToast("Module updated successfully.");
    });

    for (const state of states) {
      await test.step(`selecting ${state.label} shows its color and persists`, async () => {
        // Re-picking the current state is a no-op with no notice.
        if ((await driver.modulesPillText("State Module")) !== state.label) {
          await driver.modulesOpenStatus("State Module");
          await driver.modulesPickStatus(state.label);
          await driver.modulesAwaitToast("Module updated successfully.");
        }
        expect(await driver.modulesPillText("State Module")).toBe(state.label);
        expect(await driver.modulesPillColor("State Module")).toBe(state.color);
        const saved = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
        expect(saved.status).toBe(state.value);
      });
    }
  }
);

test(
  specTitle(ROW_015, "lead avatar shows when set and a no-lead marker otherwise"),
  { tag: specTags(ROW_015) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod015");
    const lead = await seatFreshMember(world.owner, world.workspaceSlug, ROLE.MEMBER, "parity-mod015l");
    await addProjectMembersViaApi(world.owner, world.workspaceSlug, world.projectId, [
      { member_id: lead.userId, role: ROLE.MEMBER },
    ]);
    await serverCreateModule(world.workspaceSlug, world.projectId, "Led Module", world.owner.cookie);
    const ownedId = await serverCreateModule(world.workspaceSlug, world.projectId, "Owned Module", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      ownedId,
      { lead_id: lead.userId },
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner, "Owned Module");

    await test.step("a module with a lead shows the lead avatar", async () => {
      expect(await driver.modulesLeadState("Owned Module")).toBe("avatar");
      expect(await driver.modulesLeadTooltip("Owned Module")).toBeNull();
    });

    await test.step("a module without a lead shows the marker with an explanation", async () => {
      expect(await driver.modulesLeadState("Led Module")).toBe("none");
      expect(await driver.modulesLeadTooltip("Led Module")).toBe("No lead");
    });
  }
);

test(
  specTitle(ROW_016, "starring toggles the favorite, notifies, and expands sidebar favorites"),
  { tag: specTags(ROW_016) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod016");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Starred Module",
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner, "Starred Module");

    await test.step("the star starts unselected with favorites collapsed", async () => {
      expect(await driver.modulesStarVisible("Starred Module")).toBe(true);
      expect(await driver.modulesStarSelected("Starred Module")).toBe(false);
      expect(await driver.modulesFavoritesExpanded()).toBe(false);
    });

    await test.step("starring selects, notifies, persists, and expands favorites", async () => {
      await driver.modulesToggleStar("Starred Module");
      await driver.modulesAwaitToast("Module added to favorites.");
      // The star flickers through a stale read while the favorite sync
      // settles; poll for its end state.
      await expect.poll(() => driver.modulesStarSelected("Starred Module"), { timeout: 30_000 }).toBe(true);
      await expect.poll(() => driver.modulesFavoritesExpanded(), { timeout: 30_000 }).toBe(true);
      const saved = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(saved.isFavorite).toBe(true);
    });

    await test.step("unstarring clears the selection and persists", async () => {
      await driver.modulesToggleStar("Starred Module");
      await driver.modulesAwaitToast("Module removed from favorites.");
      await expect.poll(() => driver.modulesStarSelected("Starred Module"), { timeout: 30_000 }).toBe(false);
      const saved = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      expect(saved.isFavorite).toBe(false);
    });
  }
);

test(specTitle(ROW_016, "guests see no favorite star"), { tag: specTags(ROW_016) }, async ({ driver }) => {
  const world = await setupModulesWorld("parity-mod016g");
  await serverCreateModule(world.workspaceSlug, world.projectId, "Guest Module", world.owner.cookie);
  const guest = await setupGuest(world, "parity-mod016gg");

  await enterModulesList(driver, world, guest, "Guest Module");
  expect(await driver.modulesStarVisible("Guest Module")).toBe(false);
});

test(
  specTitle(ROW_017, "row menu assembles per role with the archive gate explained"),
  { tag: specTags(ROW_017) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod017");
    const activeId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Active Module",
      world.owner.cookie
    );
    void activeId;
    const doneId = await serverCreateModule(world.workspaceSlug, world.projectId, "Done Module", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      doneId,
      { status: "completed" },
      world.owner.cookie
    );
    const archivedId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Gone Module",
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      archivedId,
      { status: "cancelled" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, archivedId, world.owner.cookie);
    const guest = await setupGuest(world, "parity-mod017g");

    await enterModulesList(driver, world, world.owner, "Active Module");

    await test.step("editors on an unfinished module see archive disabled with an explanation", async () => {
      await driver.modulesOpenRowMenu("Active Module");
      const entries = await driver.modulesMenuItems();
      expect(entries.map((entry) => entry.title)).toEqual([
        "Edit",
        "Open in new tab",
        "Copy link",
        "Archive",
        "Delete",
      ]);
      const archive = entries.find((entry) => entry.title === "Archive");
      expect(archive?.disabled).toBe(true);
      expect(archive?.description ?? "").toContain("completed or cancelled");
      await driver.modulesMenuChoose("Copy link");
    });

    await test.step("editors on a finished module see archive enabled", async () => {
      await driver.modulesOpenRowMenu("Done Module");
      const entries = await driver.modulesMenuItems();
      expect(entries.find((entry) => entry.title === "Archive")?.disabled).toBe(false);
      await driver.modulesMenuChoose("Copy link");
    });

    await test.step("guests see only open and copy", async () => {
      await enterModulesList(driver, world, guest, "Active Module");
      await driver.modulesOpenRowMenu("Active Module");
      expect((await driver.modulesMenuItems()).map((entry) => entry.title)).toEqual(["Open in new tab", "Copy link"]);
      await driver.modulesMenuChoose("Copy link");
    });

    await test.step("editors on an archived module see restore instead of edit/archive/delete", async () => {
      await driver.modulesOpenList(world.workspaceSlug, world.projectId, browserSessionCookies(world.owner));
      await openArchivesModulesTab(driver, world.workspaceSlug, world.projectId);
      await driver.modulesAwaitRow("Gone Module");
      await driver.modulesOpenRowMenu("Gone Module");
      const entries = await driver.modulesMenuItems();
      expect(entries.map((entry) => entry.title)).toEqual(["Open in new tab", "Copy link", "Restore"]);
      expect(entries.find((entry) => entry.title === "Restore")?.disabled).toBe(false);
      await driver.modulesMenuChoose("Copy link");
    });
  }
);

test(
  specTitle(ROW_017, "each row menu entry launches its behavior"),
  { tag: specTags(ROW_017) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod017l");
    await serverCreateModule(world.workspaceSlug, world.projectId, "Launch Module", world.owner.cookie);
    const doneId = await serverCreateModule(world.workspaceSlug, world.projectId, "Launch Done", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      doneId,
      { status: "completed" },
      world.owner.cookie
    );
    const archivedId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Launch Gone",
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      archivedId,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, archivedId, world.owner.cookie);

    await enterModulesList(driver, world, world.owner, "Launch Module");

    await test.step("edit opens the edit dialog", async () => {
      await driver.modulesOpenRowMenu("Launch Module");
      await driver.modulesMenuChoose("Edit");
      expect(await driver.modulesDialogVisible("edit")).toBe(true);
      expect(await driver.modulesDialogHeading()).toContain("Update module");
      await enterModulesList(driver, world, world.owner, "Launch Module");
    });

    await test.step("delete opens the delete dialog", async () => {
      await driver.modulesOpenRowMenu("Launch Module");
      await driver.modulesMenuChoose("Delete");
      expect(await driver.modulesDialogVisible("delete")).toBe(true);
      expect(await driver.modulesDialogHeading()).toContain("Delete module");
      await enterModulesList(driver, world, world.owner, "Launch Module");
    });

    await test.step("archive on a finished module opens the archive dialog", async () => {
      await driver.modulesOpenRowMenu("Launch Done");
      await driver.modulesMenuChoose("Archive");
      expect(await driver.modulesDialogVisible("archive")).toBe(true);
      expect(await driver.modulesDialogHeading()).toContain("Launch Done");
      await enterModulesList(driver, world, world.owner, "Launch Module");
    });

    await test.step("restore on an archived module restores with a notice", async () => {
      await driver.modulesOpenList(world.workspaceSlug, world.projectId, browserSessionCookies(world.owner));
      await openArchivesModulesTab(driver, world.workspaceSlug, world.projectId);
      await driver.modulesAwaitRow("Launch Gone");
      await driver.modulesOpenRowMenu("Launch Gone");
      await driver.modulesMenuChoose("Restore");
      await driver.modulesAwaitToast("Your module can be found in project modules.");
      expect(await driver.modulesCurrentUrl()).toContain("/archives/modules");
    });
  }
);

test(
  specTitle(ROW_018, "copy link writes the full module URL and open-in-new-tab loads it standalone"),
  { tag: specTags(ROW_018) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod018");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Linked Module",
      world.owner.cookie
    );
    // The copy carries the serving origin, which differs per target.
    const origin = (driver.target === "web_new" ? parityNewUrl(process.env) : parityOracleUrl(process.env)).replace(
      /\/+$/,
      ""
    );
    const expectedUrl = `${origin}/${world.workspaceSlug}/projects/${world.projectId}/modules/${moduleId}`;

    await enterModulesList(driver, world, world.owner, "Linked Module");

    await test.step("copy writes the full URL with a notice", async () => {
      await driver.modulesOpenRowMenu("Linked Module");
      await driver.modulesMenuChoose("Copy link");
      await driver.modulesAwaitToast("Module link copied to clipboard.");
      expect(await driver.modulesReadClipboard()).toBe(expectedUrl);
    });

    await test.step("open-in-new-tab loads the same module standalone", async () => {
      await driver.modulesOpenRowMenu("Linked Module");
      const popup = await driver.modulesMenuChooseOpenTab("Linked Module");
      expect(popup.url).toContain(`/projects/${world.projectId}/modules/${moduleId}`);
      expect(popup.title).toContain("Linked Module");
    });
  }
);

test(
  specTitle(ROW_018, "detail header menu copies the link and opens the module standalone"),
  { tag: specTags(ROW_018) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod018d");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Detail Module",
      world.owner.cookie
    );
    // The copy carries the serving origin, which differs per target.
    const origin = (driver.target === "web_new" ? parityNewUrl(process.env) : parityOracleUrl(process.env)).replace(
      /\/+$/,
      ""
    );
    const expectedUrl = `${origin}/${world.workspaceSlug}/projects/${world.projectId}/modules/${moduleId}`;

    await driver.modulesOpenDetail(world.workspaceSlug, world.projectId, moduleId, browserSessionCookies(world.owner));

    await test.step("copy writes the full URL with a notice", async () => {
      await driver.modulesDetailOpenMenu();
      await driver.modulesMenuChoose("Copy link");
      await driver.modulesAwaitToast("Module link copied to clipboard.");
      expect(await driver.modulesReadClipboard()).toBe(expectedUrl);
    });

    await test.step("open-in-new-tab loads the same module standalone", async () => {
      await driver.modulesDetailOpenMenu();
      const popup = await driver.modulesMenuChooseOpenTab("Detail Module");
      expect(popup.url).toContain(`/projects/${world.projectId}/modules/${moduleId}`);
      expect(popup.title).toContain("Detail Module");
    });
  }
);

test(
  specTitle(ROW_019, "peek panel opens from the row, drives the URL param, and closes cleanly"),
  { tag: specTags(ROW_019) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod019");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Peek Module", world.owner.cookie);

    await enterModulesList(driver, world, world.owner, "Peek Module");

    await test.step("the info control opens the panel and sets the param", async () => {
      expect(await driver.modulesPeekVisible()).toBe(false);
      await driver.modulesTogglePeek("Peek Module");
      expect(await driver.modulesPeekVisible()).toBe(true);
      expect(await driver.modulesPeekName()).toBe("Peek Module");
      expect(await driver.modulesCurrentUrl()).toContain(`peekModule=${moduleId}`);
    });

    await test.step("closing removes the param and the panel", async () => {
      await driver.modulesClosePeek();
      expect(await driver.modulesPeekVisible()).toBe(false);
      expect(await driver.modulesCurrentUrl()).not.toContain("peekModule");
    });
  }
);

test(
  specTitle(ROW_019, "a peek URL reopens the same panel fresh and adapts the board density"),
  { tag: specTags(ROW_019) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod019s");
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Share Peek", world.owner.cookie);

    await enterModulesList(driver, world, world.owner, "Share Peek");
    await driver.modulesSetLayout("board");
    const closedColumns = await driver.modulesBoardColumnCount();

    await test.step("opening peek narrows the board grid", async () => {
      await driver.modulesSetLayout("list");
      await driver.modulesAwaitRow("Share Peek");
      await driver.modulesTogglePeek("Share Peek");
      expect(await driver.modulesPeekVisible()).toBe(true);
      await driver.modulesSetLayout("board");
      const openColumns = await driver.modulesBoardColumnCount();
      expect(openColumns).toBeLessThan(closedColumns);
      await driver.modulesClosePeek();
    });

    await test.step("the peek URL is shareable", async () => {
      await driver.openAuthenticated(
        `/${world.workspaceSlug}/projects/${world.projectId}/modules?peekModule=${moduleId}`,
        browserSessionCookies(world.owner)
      );
      await expect.poll(() => driver.modulesPeekVisible(), { timeout: 30_000 }).toBe(true);
      expect(await driver.modulesPeekName()).toBe("Share Peek");
    });
  }
);

test(
  specTitle(ROW_019, "clicking an archived row opens its peek panel"),
  { tag: specTags(ROW_019) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod019a");
    const moduleId = await serverCreateModule(
      world.workspaceSlug,
      world.projectId,
      "Archived Peek",
      world.owner.cookie
    );
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      moduleId,
      { status: "completed" },
      world.owner.cookie
    );
    await archivesArchiveModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);

    await driver.modulesOpenList(world.workspaceSlug, world.projectId, browserSessionCookies(world.owner));
    await openArchivesModulesTab(driver, world.workspaceSlug, world.projectId);
    await driver.modulesAwaitRow("Archived Peek");

    await driver.modulesClickRow("Archived Peek");
    await expect.poll(() => driver.modulesPeekVisible(), { timeout: 30_000 }).toBe(true);
    expect(await driver.modulesPeekName()).toBe("Archived Peek");
    expect(await driver.modulesCurrentUrl()).toContain(`peekModule=${moduleId}`);
  }
);

test(
  specTitle(ROW_021, "timeline rows, status-colored bars, and tooltips render from saved dates"),
  { tag: specTags(ROW_021) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod021");
    const from = new Date();
    from.setDate(from.getDate() + 10);
    const to = new Date();
    to.setDate(to.getDate() + 17);
    const plannedId = await serverCreateModule(world.workspaceSlug, world.projectId, "Planned Bar", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      plannedId,
      { status: "planned", start_date: toPayloadDate(from), target_date: toPayloadDate(to) },
      world.owner.cookie
    );
    const doneId = await serverCreateModule(world.workspaceSlug, world.projectId, "Done Bar", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      doneId,
      { status: "completed", start_date: toPayloadDate(from), target_date: toPayloadDate(to) },
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner, "Planned Bar");
    await driver.modulesSetLayout("timeline");

    await test.step("sidebar rows link to detail with a status glyph", async () => {
      const rows = await driver.modulesTimelineRows();
      const names = rows.map((row) => row.name);
      expect(names).toContain("Planned Bar");
      expect(names).toContain("Done Bar");
      for (const row of rows) {
        expect(row.href).toContain(`/projects/${world.projectId}/modules/`);
        expect(row.statusGlyph).toBe(true);
      }
    });

    await test.step("bars span the saved dates in the status color", async () => {
      expect(await driver.modulesTimelineBarExists("Planned Bar")).toBe(true);
      expect(await driver.modulesTimelineBarColor("Planned Bar")).toBe("#3f76ff");
      expect(await driver.modulesTimelineBarColor("Done Bar")).toBe("#16a34a");
    });

    await test.step("the bar tooltip carries the name and date summary", async () => {
      await driver.modulesTimelineHoverBar("Planned Bar");
      const tooltip = await driver.modulesTimelineTooltip();
      expect(tooltip ?? "").toContain("Planned Bar");
      expect(tooltip ?? "").toContain(" to ");
    });
  }
);

test(
  specTitle(ROW_021, "timeline bars drag to new dates and stretch from either end"),
  { tag: specTags(ROW_021) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod021d");
    const from = new Date();
    from.setDate(from.getDate() + 10);
    const to = new Date();
    to.setDate(to.getDate() + 17);
    const moduleId = await serverCreateModule(world.workspaceSlug, world.projectId, "Drag Bar", world.owner.cookie);
    await archivesPatchModule(
      world.workspaceSlug,
      world.projectId,
      moduleId,
      { status: "planned", start_date: toPayloadDate(from), target_date: toPayloadDate(to) },
      world.owner.cookie
    );

    await enterModulesList(driver, world, world.owner, "Drag Bar");
    await driver.modulesSetLayout("timeline");
    expect(await driver.modulesTimelineBarExists("Drag Bar")).toBe(true);

    await test.step("dragging the bar shifts both dates", async () => {
      await driver.modulesTimelineDragBar("Drag Bar", 3);
      const moved = new Date(from);
      moved.setDate(moved.getDate() + 3);
      const movedEnd = new Date(to);
      movedEnd.setDate(movedEnd.getDate() + 3);
      await expect
        .poll(
          async () =>
            modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie).then(
              (detail) => `${detail.startDate} ${detail.targetDate}`
            ),
          { timeout: 30_000 }
        )
        .toBe(`${toPayloadDate(moved)} ${toPayloadDate(movedEnd)}`);
    });

    await test.step("stretching from the right moves only the end date", async () => {
      // Each mutation starts from a settled render: prior drags leave the
      // chart scrolled with the bar half under the sticky sidebar.
      await driver.modulesSetLayout("timeline");
      const before = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      await driver.modulesTimelineResizeBar("Drag Bar", "right", 2);
      const end = new Date(`${before.targetDate ?? toPayloadDate(to)}T12:00:00`);
      end.setDate(end.getDate() + 2);
      await expect
        .poll(
          async () =>
            modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie).then(
              (detail) => `${detail.startDate} ${detail.targetDate}`
            ),
          { timeout: 30_000 }
        )
        .toBe(`${before.startDate} ${toPayloadDate(end)}`);
    });

    await test.step("stretching from the left moves only the start date", async () => {
      await driver.modulesSetLayout("timeline");
      const before = await modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie);
      await driver.modulesTimelineResizeBar("Drag Bar", "left", -2);
      const start = new Date(`${before.startDate ?? toPayloadDate(from)}T12:00:00`);
      start.setDate(start.getDate() - 2);
      await expect
        .poll(
          async () =>
            modulesFetchModule(world.workspaceSlug, world.projectId, moduleId, world.owner.cookie).then(
              (detail) => `${detail.startDate} ${detail.targetDate}`
            ),
          { timeout: 30_000 }
        )
        .toBe(`${toPayloadDate(start)} ${before.targetDate}`);
    });
  }
);

test(
  specTitle(ROW_021, "timeline reorder needs manual sort and edit permission"),
  { tag: specTags(ROW_021) },
  async ({ driver }) => {
    const world = await setupModulesWorld("parity-mod021r");
    const from = new Date();
    from.setDate(from.getDate() + 10);
    const to = new Date();
    to.setDate(to.getDate() + 17);
    const ids: Record<string, string> = {};
    for (const name of ["Reorder Alpha", "Reorder Bravo", "Reorder Charlie"]) {
      const id = await serverCreateModule(world.workspaceSlug, world.projectId, name, world.owner.cookie);
      ids[name] = id;
      await archivesPatchModule(
        world.workspaceSlug,
        world.projectId,
        id,
        { start_date: toPayloadDate(from), target_date: toPayloadDate(to) },
        world.owner.cookie
      );
    }
    const guest = await setupGuest(world, "parity-mod021g");

    await enterModulesList(driver, world, world.owner, "Reorder Alpha");
    await driver.modulesSetLayout("timeline");

    await test.step("rows reorder under manual sort and persist", async () => {
      await driver.modulesSetSort("Manual");
      expect(await driver.modulesSortLabel()).toBe("Manual");
      await driver.modulesSetLayout("timeline");
      // Fresh modules share the default order, newest first.
      expect(await driver.modulesTimelineOrder()).toEqual(["Reorder Charlie", "Reorder Bravo", "Reorder Alpha"]);
      await driver.modulesTimelineDragRowBefore("Reorder Alpha", "Reorder Charlie");
      expect(await driver.modulesTimelineOrder()).toEqual(["Reorder Alpha", "Reorder Charlie", "Reorder Bravo"]);
      const names = ["Reorder Alpha", "Reorder Charlie", "Reorder Bravo"];
      const readOrders = (): Promise<number[]> =>
        Promise.all(
          names.map((name) =>
            modulesFetchModule(world.workspaceSlug, world.projectId, ids[name] ?? "", world.owner.cookie).then(
              (detail) => detail.sortOrder
            )
          )
        );
      // The UI reorders optimistically; the order PATCH lands after.
      await expect.poll(async () => readOrders().then((orders) => new Set(orders).size), { timeout: 30_000 }).toBe(3);
      const orders = await readOrders();
      expect(orders[0] < orders[1] && orders[1] < orders[2]).toBe(true);
    });

    await test.step("rows refuse to reorder under name sort", async () => {
      await driver.modulesSetSort("Name");
      await driver.modulesSetLayout("timeline");
      const before = await driver.modulesTimelineOrder();
      expect(before).toEqual(["Reorder Alpha", "Reorder Bravo", "Reorder Charlie"]);
      await driver.modulesTimelineAttemptRowBefore("Reorder Charlie", "Reorder Alpha");
      expect(await driver.modulesTimelineOrder()).toEqual(before);
    });

    await test.step("a guest drag attempt moves no bar", async () => {
      await driver.modulesOpenList(world.workspaceSlug, world.projectId, browserSessionCookies(guest));
      await driver.modulesSetLayout("timeline");
      expect(await driver.modulesTimelineBarExists("Reorder Alpha")).toBe(true);
      const alphaId = ids["Reorder Alpha"] ?? "";
      const before = await modulesFetchModule(world.workspaceSlug, world.projectId, alphaId, world.owner.cookie);
      await driver.modulesTimelineAttemptBarMove("Reorder Alpha", 3);
      const after = await modulesFetchModule(world.workspaceSlug, world.projectId, alphaId, world.owner.cookie);
      expect(`${after.startDate} ${after.targetDate}`).toBe(`${before.startDate} ${before.targetDate}`);
    });
  }
);
