// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-184): editing a definition pre-fills the dialog,
// persists changes and refreshes the list, while the handle stays locked;
// later firings use the updated definition. Row: AGT-003.
// Note: the scratch stack runs no firing beat, so "later firings" is proven
// through the binding detail's resolved prompt, which the server composes
// from the live definition at read time — an edit that moves it moves every
// future firing.
import { test, expect } from "../../fixtures";
import {
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  serverBindingDetail,
  serverCreateBinding,
  serverSchedulers,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-003"];

test(
  specTitle(ROWS, "edit pre-fills and persists with the handle locked; firings follow the update"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt3"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const slug = `agt3-edit-${workspaceSlug}`;

    const { projectId, bindingId } = await test.step("owner prepares a definition with an install", async () => {
      const project = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT3 Project ${tag}`,
        parityProjectIdentifier("AG3")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug,
        name: "AGT3 Before Edit",
        description: "Before.",
        prompt: "AGT3-ORIGINAL-PROMPT",
        color: "#3b82f6",
        is_enabled: true,
      });
      const binding = await serverCreateBinding(workspaceSlug, project.id, ownerSession, {
        scheduler: definition.id,
        project: project.id,
        dtstart: new Date(Date.now() + 24 * 3600_000).toISOString(),
        rrule: "FREQ=DAILY",
      });
      return { projectId: project.id, bindingId: binding.id };
    });

    await test.step("owner signs in and opens the catalog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenCatalog(workspaceSlug);
    });

    await test.step("the edit dialog pre-fills and locks the handle", async () => {
      await driver.schedulerOpenEdit(slug);
      expect(await driver.schedulerDefinitionValues()).toMatchObject({
        name: "AGT3 Before Edit",
        handle: slug,
        description: "Before.",
        prompt: "AGT3-ORIGINAL-PROMPT",
        color: "#3b82f6",
        enabled: true,
      });
      expect(await driver.schedulerDefinitionHandleLocked()).toBe(true);
    });

    await test.step("saving persists, refreshes and keeps the handle", async () => {
      await driver.schedulerFillDefinition({
        name: "AGT3 After Edit",
        description: "After.",
        prompt: "AGT3-UPDATED-PROMPT",
        color: "#ef4444",
      });
      await driver.schedulerSetDefinitionEnabled(false);
      await driver.schedulerSubmitDefinition();
      await expect
        .poll(() => driver.rulesLastToast(), { timeout: 60_000 })
        .toEqual({ title: "Scheduler updated", message: expect.stringContaining("updated definition") });
      expect(await driver.schedulerDefinitionOpen()).toBe(false);
      await expect
        .poll(async () => (await driver.schedulerCatalogRows()).map((row) => row.name), { timeout: 60_000 })
        .toContain("AGT3 After Edit");
      const stored = (await serverSchedulers(workspaceSlug, ownerSession)).find((row) => row.slug === slug);
      expect(stored).toMatchObject({
        name: "AGT3 After Edit",
        description: "After.",
        prompt: "AGT3-UPDATED-PROMPT",
        color: "#ef4444",
        is_enabled: false,
      });
      const rows = await driver.schedulerCatalogRows();
      expect(rows.find((row) => row.handle === slug)?.status).toBe("Disabled");
    });

    await test.step("later firings resolve against the updated definition", async () => {
      const detail = await serverBindingDetail(workspaceSlug, projectId, bindingId, ownerSession);
      expect(detail.resolved_prompt).toContain("AGT3-UPDATED-PROMPT");
      expect(detail.resolved_prompt).not.toContain("AGT3-ORIGINAL-PROMPT");
    });
  }
);
