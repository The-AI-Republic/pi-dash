// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): the workspace scheduler catalog lists
// every definition with its origin, install count, enabled state and last
// update; members and guests can view but not mutate; an emptied catalog
// shows a guidance row; the tab title carries the workspace name.
// Row: AGT-001.
// Note: only the Built-in origin mark is provable — nothing in the API
// creates manifest-source definitions (the serializer leaves `source`
// read-only and no loader sets it), so no manifest row can be provisioned.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindings,
  serverCreateBinding,
  serverDeleteScheduler,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatGuest, seatMember } from "../support";

const ROWS = ["AGT-001"];

test(
  specTitle(ROWS, "catalog lists definitions for every role; emptied catalog guides"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt1"));
    const { owner, ownerSession, workspaceSlug, workspaceName } = harness;

    const project = await test.step("owner prepares a project and definitions", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT1 Project ${workspaceSlug}`,
        parityProjectIdentifier("AG1")
      );
      const enabled = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt1-enabled-${workspaceSlug}`,
        name: "AGT1 Enabled Definition",
        description: "Installed once, so the count cell is live.",
        prompt: "Audit this project nightly.",
        color: "#8b5cf6",
        is_enabled: true,
      });
      await ensureScheduler(workspaceSlug, ownerSession, {
        slug: `agt1-disabled-${workspaceSlug}`,
        name: "AGT1 Disabled Definition",
        prompt: "Paused for now.",
        is_enabled: false,
      });
      await serverCreateBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: enabled.id,
        project: created.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return created;
    });

    await test.step("owner signs in and opens the catalog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
    });

    await test.step("every definition renders with origin, installs, status and update time", async () => {
      const server = await serverSchedulers(workspaceSlug, ownerSession);
      const rows = await driver.schedulerCatalogRows();
      expect(rows.map((row) => row.handle).sort()).toEqual(server.map((row) => row.slug).sort());
      // Display order follows the API order (by name).
      expect(rows.map((row) => row.handle)).toEqual(server.map((row) => row.slug));
      for (const row of rows) {
        const match = server.find((candidate) => candidate.slug === row.handle);
        expect(match).toBeDefined();
        expect(row.name).toBe(match?.name ?? "");
        expect(row.origin).toBe("Built-in");
        expect(row.installs).toContain(String(match?.active_binding_count ?? -1));
        expect(row.status).toBe((match?.is_enabled ?? false) ? "Enabled" : "Disabled");
        expect(row.updated.length).toBeGreaterThan(0);
      }
      const enabledRow = rows.find((row) => row.handle === `agt1-enabled-${workspaceSlug}`);
      expect(enabledRow?.installs).toContain("1");
      expect(await driver.schedulerCatalogEmptyVisible()).toBe(false);
    });

    await test.step("actions match the definition state; title carries the workspace", async () => {
      expect(await driver.schedulerCreateVisible()).toBe(true);
      expect(await driver.schedulerRowActions(`agt1-enabled-${workspaceSlug}`)).toEqual(["Install", "Edit", "Delete"]);
      // Disabled definitions cannot be installed: no per-row install action.
      expect(await driver.schedulerRowActions(`agt1-disabled-${workspaceSlug}`)).toEqual(["Edit", "Delete"]);
      const title = await driver.schedulerPageTitle();
      expect(title).toContain(workspaceName);
      expect(title).toContain("Schedulers");
    });

    await test.step("member views the same catalog without mutation controls", async () => {
      const member = await seatMember(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
      const server = await serverSchedulers(workspaceSlug, ownerSession);
      const rows = await driver.schedulerCatalogRows();
      expect(rows.map((row) => row.handle).sort()).toEqual(server.map((row) => row.slug).sort());
      expect(await driver.schedulerCreateVisible()).toBe(false);
      expect(await driver.schedulerRowActions(`agt1-enabled-${workspaceSlug}`)).toEqual([]);
    });

    await test.step("guest views the same catalog without mutation controls", async () => {
      const guest = await seatGuest(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(guest.email, guest.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
      const server = await serverSchedulers(workspaceSlug, ownerSession);
      const rows = await driver.schedulerCatalogRows();
      expect(rows.map((row) => row.handle).sort()).toEqual(server.map((row) => row.slug).sort());
      expect(await driver.schedulerCreateVisible()).toBe(false);
      expect(await driver.schedulerRowActions(`agt1-enabled-${workspaceSlug}`)).toEqual([]);
    });

    await test.step("an emptied catalog shows the guidance row", async () => {
      for (const row of await serverSchedulers(workspaceSlug, ownerSession)) {
        await serverDeleteScheduler(workspaceSlug, row.id, ownerSession);
      }
      expect(await serverSchedulers(workspaceSlug, ownerSession)).toEqual([]);
      const bindings = await serverBindings(workspaceSlug, project.id, ownerSession);
      expect(bindings).toEqual([]);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
      expect(await driver.schedulerCatalogEmptyVisible()).toBe(true);
      expect(await driver.schedulerCatalogRows()).toEqual([]);
    });
  }
);
