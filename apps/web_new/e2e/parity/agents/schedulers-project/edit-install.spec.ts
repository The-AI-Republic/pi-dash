// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-185): editing an install pre-fills every field
// with the stored start converted to the local input; saving persists and
// refreshes the caller; untouched rules round-trip byte-identical; backend
// field errors surface without closing; an emptied pod persists as the
// project default.
// Row: AGT-010.
// Note: the row's "reconstructs builder state ... opening in raw mode for
// inexpressible rules" half is unprovable — this checkout has no RRULE
// builder widget (a plain textarea pre-fills verbatim instead); the builder
// is the main#561 target rowed under AGT-065.
import { test, expect } from "../../fixtures";
import {
  createPod,
  ensureBinding,
  ensureProject,
  ensureScheduler,
  parityProjectIdentifier,
  projectPods,
  serverBindingDetail,
  serverBindings,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-010"];

/** Poll visible toasts until one carries `title`; resolves with its message. */
async function toastMessage(
  driver: { schedulerVisibleToasts: () => Promise<{ title: string; message: string }[]> },
  title: string
): Promise<string> {
  let message = "";
  await expect
    .poll(
      async () => {
        const found = (await driver.schedulerVisibleToasts()).find((toast) => toast.title === title);
        message = found?.message ?? "";
        return message;
      },
      { timeout: 30_000 }
    )
    .not.toBe("");
  return message;
}

test(
  specTitle(ROWS, "edit pre-fills locally, persists, refreshes callers, surfaces backend errors"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-ag10"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;
    const handle = `agt10-main-${workspaceSlug}`;

    const project = await test.step("owner prepares a project with one install", async () => {
      const created = await ensureProject(
        workspaceSlug,
        ownerSession,
        `AGT10 Project ${tag}`,
        parityProjectIdentifier("AG10")
      );
      const definition = await ensureScheduler(workspaceSlug, ownerSession, {
        slug: handle,
        name: "AGT10 Main Definition",
        prompt: "Audit this project nightly.",
        is_enabled: true,
      });
      const dtstart = new Date(Date.now() + 2 * 24 * 3600_000).toISOString();
      await ensureBinding(workspaceSlug, created.id, ownerSession, {
        scheduler: definition.id,
        project: created.id,
        dtstart,
        tzid: "Europe/Berlin",
        rrule: "FREQ=WEEKLY;BYDAY=MO,WE",
        extra_context: "AGT10 original framing.",
      });
      return created;
    });
    // Created before any dialog opens: the pod selector reads its options
    // once, when its dialog mounts.
    const pod = await test.step("owner adds a project pod", async () => {
      const pods = await projectPods(project.id, ownerSession);
      const name = `agt10rig${tag.slice(0, 8)}`;
      const existing = pods.find((row) => row.name.endsWith(name));
      if (existing !== undefined) return existing;
      return createPod(project.id, ownerSession, name);
    });
    const bindingId = async (): Promise<string> => {
      const found = (await serverBindings(workspaceSlug, project.id, ownerSession)).find(
        (row) => row.scheduler_slug === handle
      );
      if (found === undefined) throw new Error("[parity] expected the AGT10 install.");
      return found.id;
    };

    await test.step("owner signs in and opens the edit dialog", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.schedulerOpenProjectList(workspaceSlug, project.id);
      await driver.schedulerProjectOpenEdit(handle);
    });

    await test.step("every field pre-fills, the stored start as a local input", async () => {
      const server = await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession);
      const values = await driver.schedulerProjectEditValues();
      // The input speaks browser-local wall time; the server speaks UTC.
      const skew = Math.abs(new Date(server.dtstart).getTime() - new Date(values.dtstart).getTime());
      expect(skew).toBeLessThan(60_000);
      expect(values.tzid).toBe("Europe/Berlin");
      expect(values.rrule).toBe("FREQ=WEEKLY;BYDAY=MO,WE");
      expect(values.extraContext).toBe("AGT10 original framing.");
      expect(values.enabled).toBe(true);
      expect(values.outcomeLabel).toBe("Create issues");
      expect(values.pod).toBe("");
      expect(await driver.schedulerProjectEditHumanizer()).toContain("Monday");
    });

    await test.step("the humanizer stays live under the RRULE field", async () => {
      await driver.schedulerProjectEditFill({ rrule: "FREQ=DAILY" });
      await expect.poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 }).toContain("day");
      await driver.schedulerProjectEditFill({ rrule: "" });
      await expect
        .poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 })
        .toBe("Fires once at the start time.");
      // An invalid rule echoes raw: the humanizer returns its input on parse
      // error, so the "Invalid RRULE" fallback string never renders.
      await driver.schedulerProjectEditFill({ rrule: "FREQ=NOPE" });
      await expect.poll(() => driver.schedulerProjectEditHumanizer(), { timeout: 30_000 }).toBe("FREQ=NOPE");
      await driver.schedulerProjectEditFill({ rrule: "FREQ=MONTHLY;BYMONTHDAY=15" });
    });

    await test.step("saving persists every field and refreshes the list", async () => {
      expect((await projectPods(project.id, ownerSession)).some((row) => row.id === pod.id)).toBe(true);
      await expect
        .poll(async () => (await driver.schedulerPodOptions()).map((row) => row.value), { timeout: 30_000 })
        .toContain(pod.id);
      await driver.schedulerProjectEditFill({
        tzid: "Asia/Tokyo",
        extraContext: "AGT10 edited framing.",
      });
      await driver.schedulerOutcomeSelect("Apply fix");
      await driver.schedulerPodSelect(pod.id);
      await driver.schedulerProjectEditSetEnabled(false);
      await driver.schedulerProjectEditSubmit();
      const message = await toastMessage(driver, "Install updated");
      expect(message).toContain("Subsequent runs use the new settings.");
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      const server = await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession);
      expect(server.tzid).toBe("Asia/Tokyo");
      expect(server.rrule).toBe("FREQ=MONTHLY;BYMONTHDAY=15");
      expect(server.extra_context).toBe("AGT10 edited framing.");
      expect(server.enabled).toBe(false);
      expect(server.outcome_mode).toBe("apply_fix");
      expect(server.pod).toBe(pod.id);
      await expect
        .poll(async () => (await driver.schedulerProjectRows()).find((candidate) => candidate.handle === handle), {
          timeout: 30_000,
        })
        .toEqual(expect.objectContaining({ status: "Disabled" }));
      const row = (await driver.schedulerProjectRows()).find((candidate) => candidate.handle === handle);
      expect(row?.schedule).toContain("month");
    });

    await test.step("an untouched rule round-trips byte-identical", async () => {
      const before = (await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession)).rrule;
      await driver.schedulerProjectOpenEdit(handle);
      expect((await driver.schedulerProjectEditValues()).rrule).toBe(before);
      await driver.schedulerProjectEditSubmit();
      await toastMessage(driver, "Install updated");
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      expect((await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession)).rrule).toBe(
        before
      );
    });

    await test.step("backend field errors surface without closing", async () => {
      await driver.schedulerProjectOpenEdit(handle);
      await driver.schedulerProjectEditFill({ rrule: "FREQ=NOPE" });
      await driver.schedulerProjectEditSubmit();
      const message = await toastMessage(driver, "Something went wrong");
      expect(message).toContain("FREQ");
      expect(await driver.schedulerProjectEditOpen()).toBe(true);
      expect((await driver.schedulerProjectEditValues()).rrule).toBe("FREQ=NOPE");
      expect((await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession)).rrule).toBe(
        "FREQ=MONTHLY;BYMONTHDAY=15"
      );
      await driver.schedulerCloseProjectEdit();
    });

    await test.step("an emptied pod persists as the project default", async () => {
      await driver.schedulerProjectOpenEdit(handle);
      const values = await driver.schedulerProjectEditValues();
      expect(values.pod).not.toBe("");
      await driver.schedulerPodSelect("");
      await driver.schedulerProjectEditSubmit();
      await toastMessage(driver, "Install updated");
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      expect((await serverBindingDetail(workspaceSlug, project.id, await bindingId(), ownerSession)).pod).toBeNull();
    });

    await test.step("saving from the detail refreshes the detail", async () => {
      await driver.schedulerOpenProjectBinding(workspaceSlug, project.id, await bindingId());
      await driver.schedulerBindingOpenEdit();
      await driver.schedulerProjectEditFill({ extraContext: "AGT10 detail-edited framing." });
      await driver.schedulerProjectEditSetEnabled(true);
      await driver.schedulerProjectEditSubmit();
      await toastMessage(driver, "Install updated");
      await expect.poll(() => driver.schedulerProjectEditOpen(), { timeout: 30_000 }).toBe(false);
      await expect
        .poll(async () => (await driver.schedulerBindingConfig()).find((row) => row.label === "Time zone")?.value, {
          timeout: 30_000,
        })
        .toBe("Asia/Tokyo");
      expect(await driver.schedulerBindingExtraContext()).toBe("AGT10 detail-edited framing.");
      const config = await driver.schedulerBindingConfig();
      expect(config.find((row) => row.label === "Enabled")?.value).toBe("Enabled");
    });
  }
);
