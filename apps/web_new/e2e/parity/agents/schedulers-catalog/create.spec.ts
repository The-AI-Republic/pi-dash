// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): creating a workspace scheduler definition
// persists, refreshes the list and shows a success notice; an invalid submit
// is rejected inline with the dialog kept open; a backend failure surfaces
// its field detail instead of closing. Row: AGT-002.
import { test, expect } from "../../fixtures";
import { serverCreateScheduler, serverSchedulers } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";

const ROWS = ["AGT-002"];

test(
  specTitle(ROWS, "create persists and refreshes; invalid and failing submits stay open"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt2"));
    const { owner, ownerSession, workspaceSlug } = harness;
    const slug = `agt2-weekly-${workspaceSlug}`;

    await test.step("owner signs in and opens the catalog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
    });

    await test.step("a valid submit persists, refreshes and shows success", async () => {
      const before = await serverSchedulers(workspaceSlug, ownerSession);
      await driver.schedulerOpenCreate();
      await driver.schedulerFillDefinition({
        name: "AGT2 Weekly Review",
        handle: slug,
        description: "A weekly review definition.",
        prompt: "Review the week's progress.",
        color: "#8b5cf6",
      });
      await driver.schedulerSubmitDefinition();
      await expect
        .poll(() => driver.rulesLastToast(), { timeout: 60_000 })
        .toEqual({ title: "Scheduler created", message: expect.stringContaining("install") });
      expect(await driver.schedulerDefinitionOpen()).toBe(false);
      await expect
        .poll(async () => (await driver.schedulerCatalogRows()).map((row) => row.handle), { timeout: 60_000 })
        .toContain(slug);
      const after = await serverSchedulers(workspaceSlug, ownerSession);
      expect(after.length).toBe(before.length + 1);
      const stored = after.find((row) => row.slug === slug);
      expect(stored).toMatchObject({
        name: "AGT2 Weekly Review",
        description: "A weekly review definition.",
        prompt: "Review the week's progress.",
        color: "#8b5cf6",
        is_enabled: true,
      });
    });

    await test.step("a malformed handle is rejected inline and keeps the dialog open", async () => {
      const before = await serverSchedulers(workspaceSlug, ownerSession);
      await driver.schedulerOpenCreate();
      await driver.schedulerFillDefinition({
        name: "AGT2 Bad Handle",
        handle: "Not A Handle!!",
        prompt: "Never stored.",
      });
      await driver.schedulerSubmitDefinition();
      await expect
        .poll(() => driver.schedulerDefinitionErrors(), { timeout: 30_000 })
        .toContain("Use lowercase letters, numbers, and dashes only.");
      expect(await driver.schedulerDefinitionOpen()).toBe(true);
      expect(await driver.schedulerCatalogRows()).toHaveLength(before.length);
      const after = await serverSchedulers(workspaceSlug, ownerSession);
      expect(after.map((row) => row.slug).sort()).toEqual(before.map((row) => row.slug).sort());
      await driver.schedulerCloseDefinition();
    });

    await test.step("a backend failure surfaces its detail and keeps the dialog open", async () => {
      await serverCreateScheduler(workspaceSlug, ownerSession, {
        slug: `agt2-taken-${workspaceSlug}`,
        name: "AGT2 Taken",
        prompt: "Holds the slug.",
      });
      await driver.schedulerOpenCreate();
      await driver.schedulerFillDefinition({
        name: "AGT2 Duplicate",
        handle: `agt2-taken-${workspaceSlug}`,
        prompt: "Collides on the slug.",
      });
      await driver.schedulerSubmitDefinition();
      await expect
        .poll(() => driver.rulesLastToast(), { timeout: 60_000 })
        .toEqual({ title: "Something went wrong", message: expect.stringContaining("already in use") });
      expect(await driver.schedulerDefinitionOpen()).toBe(true);
      const stored = (await serverSchedulers(workspaceSlug, ownerSession)).filter(
        (row) => row.slug === `agt2-taken-${workspaceSlug}`
      );
      expect(stored).toHaveLength(1);
      await driver.schedulerCloseDefinition();
    });

    await test.step("the create control stays hidden from non-admins", async () => {
      const member = await seatMember(harness);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
      expect(await driver.schedulerCreateVisible()).toBe(false);
    });
  }
);
