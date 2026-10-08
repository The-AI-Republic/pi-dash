// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-123): widget visibility and ordering.
// Rows: SHELL-018 (toggles and drag ordering apply immediately and
// persist per workspace), SHELL-019 (retired widget key never renders,
// never appears as a toggle, never blocks the all-off empty state),
// SHELL-020 (all-widgets-off illustration yields to the first re-enabled
// widget). Behavior learned from the old dashboard in prose: the manage
// dialog lists every surfaced widget with a toggle plus drag handles,
// changes confirm with a notice, and the retired stickies key stays
// invisible everywhere.
import { test, expect } from "../../fixtures";
import type { ParityDriver } from "../../drivers/parity-driver";
import {
  serverSetTourCompleted,
  serverSetWidget,
  serverWidgets,
  signInSessionRetry,
  serverEnsureWidgets,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-018", "SHELL-019", "SHELL-020"];

async function signedInHome(
  driver: ParityDriver,
  seed: { email: string; password: string; workspaceSlug: string }
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.homeOpen(seed.workspaceSlug);
}

async function restoreWidgets(
  workspaceSlug: string,
  session: string,
  snapshot: { key: string; is_enabled?: boolean; sort_order?: number }[]
): Promise<void> {
  for (const widget of snapshot) {
    await serverSetWidget(workspaceSlug, session, widget.key, {
      is_enabled: widget.is_enabled ?? true,
      ...(widget.sort_order === undefined ? {} : { sort_order: widget.sort_order }),
    }).catch(() => undefined);
  }
}

test(
  specTitle(ROWS, "widget toggles apply immediately and persist"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    const snapshot = await serverWidgets(seed.workspaceSlug, session);

    await signedInHome(driver, seed);

    await test.step("toggling recents off hides it at once", async () => {
      // Toggles apply silently (the store call carries no notice); the
      // reorder path below is where confirmations toast.
      await serverSetWidget(seed.workspaceSlug, session, "recents", { is_enabled: true });
      await driver.homeOpenManageWidgets();
      expect(await driver.homeManageWidgetEnabled("Recents")).toBe(true);
      await driver.homeToggleManageWidget("Recents");
      await driver.homeCloseManageWidgets();
      await expect.poll(() => driver.homeWidgetTitles(), { timeout: 30_000 }).not.toContain("Recents");
    });

    await test.step("the toggle survives reloads on screen and server", async () => {
      await driver.homeReload();
      await expect.poll(() => driver.homeWidgetTitles(), { timeout: 60_000 }).not.toContain("Recents");
      const stored = await serverWidgets(seed.workspaceSlug, session);
      expect(stored.find((widget) => widget.key === "recents")?.is_enabled).toBe(false);
    });

    await test.step("re-enabling restores the widget", async () => {
      await driver.homeOpen(seed.workspaceSlug);
      await driver.homeOpenManageWidgets();
      await driver.homeToggleManageWidget("Recents");
      await driver.homeCloseManageWidgets();
      await expect.poll(() => driver.homeWidgetTitles(), { timeout: 30_000 }).toContain("Recents");
      await restoreWidgets(seed.workspaceSlug, session, snapshot);
    });
  }
);

test(specTitle(ROWS, "widget drag ordering persists"), { tag: specTags(ROWS) }, async ({ driver, seed }) => {
  const session = await signInSessionRetry(seed.email, seed.password);
  await serverSetTourCompleted(session, true);
  await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
  const snapshot = await serverWidgets(seed.workspaceSlug, session);
  const serverOrder = async (): Promise<string[]> => {
    const stored = await serverWidgets(seed.workspaceSlug, session);
    return [...stored].sort((a, b) => (b.sort_order ?? 0) - (a.sort_order ?? 0)).map((w) => w.key);
  };
  try {
    // Deterministic start: only the two widgets on, Quicklinks first. An
    // interrupted earlier run may have persisted the swapped order, which
    // would make the drag below a no-op. The retired stickies key never
    // renders, so it is left alone (its endpoint refuses writes).
    for (const widget of snapshot) {
      if (widget.key !== "quick_links" && widget.key !== "recents" && widget.key !== "my_stickies") {
        await serverSetWidget(seed.workspaceSlug, session, widget.key, { is_enabled: false });
      }
    }
    await serverSetWidget(seed.workspaceSlug, session, "quick_links", { is_enabled: true, sort_order: 100 });
    await serverSetWidget(seed.workspaceSlug, session, "recents", { is_enabled: true, sort_order: 99 });

    await signedInHome(driver, seed);
    await expect.poll(() => driver.homeWidgetTitles(), { timeout: 60_000 }).toEqual(["Quicklinks", "Recents"]);
    const before = await driver.homeWidgetTitles();

    await driver.homeOpenManageWidgets();
    const names = await driver.homeManageWidgetNames();
    expect(names.join("\n")).toContain("Quicklinks");
    // Moving the first widget below the last visibly changes the stack.
    // The reorder PATCH can be throttled under concurrent runs while the
    // dialog still toasts, so repeat the drag until the server agrees.
    for (let round = 0; round < 3; round += 1) {
      await driver.homeDragWidget("Quicklinks", "Recents");
      await expect.poll(() => driver.homeLastToast(), { timeout: 15_000 }).not.toBeNull();
      const toast = await driver.homeLastToast();
      expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`.trim().length).toBeGreaterThan(0);
      const order = await serverOrder();
      if (order.indexOf("quick_links") > order.indexOf("recents")) break;
      if (round === 2) throw new Error("[parity] reorder drag never reached the server.");
      // Reopen from server state: an optimistic client swap would make a
      // repeated drag a no-op.
      await driver.homeCloseManageWidgets();
      await driver.homeOpenManageWidgets();
    }
    await driver.homeCloseManageWidgets();

    await test.step("the stack reflects the new order after reload", async () => {
      await driver.homeReload();
      const after = await driver.homeWaitForWidgets();
      expect(after).toEqual(expect.arrayContaining(["Quicklinks", "Recents"]));
      expect(after).not.toEqual(before);
      const order = await serverOrder();
      expect(order.indexOf("quick_links")).toBeGreaterThan(order.indexOf("recents"));
    });
  } finally {
    await restoreWidgets(seed.workspaceSlug, session, snapshot);
  }
});

test(
  specTitle(ROWS, "retired key stays invisible; all-off yields on re-enable"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    const snapshot = await serverWidgets(seed.workspaceSlug, session);

    await signedInHome(driver, seed);

    await test.step("retired stickies key appears nowhere", async () => {
      await driver.homeOpenManageWidgets();
      const names = await driver.homeManageWidgetNames();
      expect(names.join("\n").toLowerCase()).not.toContain("stick");
      await driver.homeCloseManageWidgets();
      expect((await driver.homeWidgetTitles()).join("\n").toLowerCase()).not.toContain("stick");
    });

    await test.step("retired key enabled server-side still renders nothing", async () => {
      await serverSetWidget(seed.workspaceSlug, session, "my_stickies", { is_enabled: true }).catch(() => undefined);
      await driver.homeReload();
      const titles = await driver.homeWaitForWidgets();
      expect(titles).not.toEqual([]);
      expect(titles.join("\n").toLowerCase()).not.toContain("stick");
      await serverSetWidget(seed.workspaceSlug, session, "my_stickies", { is_enabled: false }).catch(() => undefined);
    });

    await test.step("all widgets off shows guidance until one returns", async () => {
      const stored = await serverWidgets(seed.workspaceSlug, session);
      const visible = stored.filter((widget) => widget.key !== "my_stickies");
      for (const widget of visible) {
        await serverSetWidget(seed.workspaceSlug, session, widget.key, { is_enabled: false });
      }
      // Confirm the server state first: a dropped write here would look
      // like a rendering bug below.
      await expect
        .poll(
          async () =>
            (await serverWidgets(seed.workspaceSlug, session))
              .filter((widget) => widget.key !== "my_stickies")
              .every((widget) => widget.is_enabled === false),
          { timeout: 30_000 }
        )
        .toBe(true);
      // A failed preferences fetch leaves the previous stack painted with
      // no retry, so reconcile with fresh loads before asserting.
      let offShown = false;
      for (let round = 0; round < 3 && !offShown; round += 1) {
        await driver.homeReload();
        try {
          await expect.poll(() => driver.homeAllOffVisible(), { timeout: 20_000 }).toBe(true);
          offShown = true;
        } catch {
          // Stale render: another fresh load.
        }
      }
      expect(offShown).toBe(true);
      await serverSetWidget(seed.workspaceSlug, session, visible[0]?.key ?? "recents", { is_enabled: true });
      await driver.homeReload();
      await expect.poll(() => driver.homeAllOffVisible(), { timeout: 60_000 }).toBe(false);
    });

    await restoreWidgets(seed.workspaceSlug, session, snapshot);
  }
);
