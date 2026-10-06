// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios for the root document shell (NEWFRONT-173).
// Behavior learned from the running old app, in prose: the shell has two
// layers. The served document carries the product title, a description,
// the social-card set, tab icons, home-screen icons and install
// manifests on every address. Once the page hydrates, the route swaps in
// its own tab title and crawler directives while the static markers
// (icons, manifests, portal roots, theme mark) stay put. Two portal roots
// sit above the provider tree for menus and editors; the theme provider
// marks the root element; toasts render through a mounted notice host;
// and with the recorder build flags absent, no third-party snippet loads
// in either layer. Row: SHELL-107.
import { test, expect } from "../../fixtures";
import {
  serverEnsureWidgets,
  serverSetTourCompleted,
  serverSetWidget,
  serverWidgets,
  signInSessionRetry,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";

const ROWS = ["SHELL-107"];

test(
  specTitle(ROWS, "document shell carries metadata, icons, manifests and portals"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    await driver.openEntry();

    await test.step("served document carries the product metadata", async () => {
      for (const path of ["/", `/${seed.workspaceSlug}/__parity_missing__`]) {
        const served = await driver.servedShellMarkers(path);
        expect(served.status, `status ${path}`).toBe(200);
        expect(served.hasTitle, `title ${path}`).toBe(true);
        expect(served.hasDescription, `description ${path}`).toBe(true);
        expect(served.hasSocial, `social ${path}`).toBe(true);
        expect(served.hasIcons, `icons ${path}`).toBe(true);
        expect(served.hasManifests, `manifests ${path}`).toBe(true);
        expect(served.hasPortals, `portals ${path}`).toBe(true);
        expect(served.hasRecorder, `recorder ${path}`).toBe(false);
      }
    });

    const facts = await driver.documentShellFacts();

    await test.step("hydrated shell keeps the route title and crawler directives", async () => {
      expect(facts.lang).toBe("en");
      expect(facts.title.trim().length).toBeGreaterThan(0);
      // Hydration swaps the served product metadata for the route pair:
      // its own tab title plus crawler directives.
      expect(facts.description).toBeNull();
      expect(facts.ogTitle).toBeNull();
      expect(facts.twitterCard).toBeNull();
      expect(facts.keywordsPresent).toBe(false);
      expect(facts.viewport ?? "").toContain("width=device-width");
      expect(facts.themeColor ?? "").not.toHaveLength(0);
      expect(facts.robots).toBe("index, nofollow");
      expect(facts.mainMounted).toBe(true);
    });

    await test.step("installability markers and resolving assets", async () => {
      expect(facts.installability.applicationName ?? "").not.toHaveLength(0);
      expect(facts.installability.appleMobileCapable).toBe("yes");
      expect(facts.installability.mobileWebCapable).toBe("yes");
      expect(facts.iconHrefs.length).toBeGreaterThanOrEqual(2);
      expect(facts.appleTouchIconHrefs.length).toBeGreaterThanOrEqual(1);
      expect(facts.manifestHrefs).toHaveLength(2);
      const statuses = await driver.installAssetStatuses();
      expect(statuses.length).toBeGreaterThan(0);
      for (const asset of statuses) {
        expect(asset.status, `asset ${asset.href}`).toBe(200);
      }
    });

    await test.step("overlay portals exist and no recorder snippet loads", async () => {
      const portals = await driver.overlayPortalsPresent();
      expect(portals.contextMenu).toBe(true);
      expect(portals.editor).toBe(true);
      expect(await driver.sessionRecorderPresent()).toBe(false);
    });
  }
);

test(
  specTitle(ROWS, "provider tree themes, hosts notices and mounts pages"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const session = await signInSessionRetry(seed.email, seed.password);
    await serverSetTourCompleted(session, true);
    await serverEnsureWidgets(seed.workspaceSlug, session, ["quick_links", "recents"]);
    const snapshot = await serverWidgets(seed.workspaceSlug, session);

    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openWorkspaceHome(seed.workspaceSlug);

      await test.step("theme provider marks the root and the shell persists", async () => {
        const facts = await driver.documentShellFacts();
        expect(["light", "dark"]).toContain(facts.rootColorScheme);
        expect(facts.mainMounted).toBe(true);
        const portals = await driver.overlayPortalsPresent();
        expect(portals.contextMenu).toBe(true);
        expect(portals.editor).toBe(true);
        expect(await driver.sessionRecorderPresent()).toBe(false);
      });

      await test.step("a reorder surfaces a notice through the toast host", async () => {
        await serverSetWidget(seed.workspaceSlug, session, "quick_links", {
          is_enabled: true,
          sort_order: 100,
        });
        await serverSetWidget(seed.workspaceSlug, session, "recents", {
          is_enabled: true,
          sort_order: 99,
        });
        await driver.homeOpen(seed.workspaceSlug);
        await expect.poll(() => driver.homeWidgetTitles(), { timeout: 60_000 }).toEqual(["Quicklinks", "Recents"]);
        await driver.homeOpenManageWidgets();
        await driver.homeDragWidget("Quicklinks", "Recents");
        await expect.poll(() => driver.homeLastToast(), { timeout: 15_000 }).not.toBeNull();
        const toast = await driver.homeLastToast();
        expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`.trim().length).toBeGreaterThan(0);
        await driver.homeCloseManageWidgets();
      });
    } finally {
      for (const widget of snapshot) {
        await serverSetWidget(seed.workspaceSlug, session, widget.key, {
          is_enabled: widget.is_enabled ?? true,
          ...(widget.sort_order === undefined ? {} : { sort_order: widget.sort_order }),
        }).catch(() => undefined);
      }
    }
  }
);
