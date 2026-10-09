// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): the custom-month dialog behind either
// duration picker requires a whole number of months in range, resets on
// cancel, and forwards valid submits to the row handler. Empty, zero,
// over-range and fractional values never persist.
// Row: AGT-036.
import { test, expect } from "../../fixtures";
import { ensureProject, parityProjectIdentifier, serverProjectAutomations } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";

const ROWS = ["AGT-036"];
const RANGE_ERROR = "Select a month between 1 and 12.";

test(
  specTitle(ROWS, "custom-month dialog bounds whole months, resets on cancel, forwards on submit"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt36"));
    const { owner, ownerSession, workspaceSlug, tag } = harness;

    const project = await test.step("owner prepares a project", async () =>
      ensureProject(workspaceSlug, ownerSession, `AGT36 Project ${tag}`, parityProjectIdentifier("AG36")));

    await test.step("owner signs in, opens automations and enables archiving", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.automationsOpen(workspaceSlug, project.id);
      await driver.automationsArchiveToggle();
      await expect.poll(async () => (await driver.automationsArchiveRow()).toggleOn, { timeout: 30_000 }).toBe(true);
    });

    await test.step("the dialog opens with a seeded input and no error", async () => {
      await driver.automationsArchiveOpenCustom();
      const modal = await driver.automationsMonthModal();
      expect(modal?.title).toBe("Customize time range");
      expect(modal?.inputValue).toBe("1");
      expect(modal?.error).toBeNull();
    });

    await test.step("empty, zero and over-range values are refused inline", async () => {
      for (const value of ["", "0", "13"]) {
        await driver.automationsMonthFill(value);
        await driver.automationsMonthSubmit();
        await expect
          .poll(async () => (await driver.automationsMonthModal())?.error ?? "", { timeout: 30_000 })
          .toBe(RANGE_ERROR);
        expect(await driver.automationsMonthModal()).not.toBeNull();
      }
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(1);
    });

    await test.step("cancel resets the input", async () => {
      await driver.automationsMonthFill("7");
      expect((await driver.automationsMonthModal())?.inputValue).toBe("7");
      await driver.automationsMonthCancel();
      expect(await driver.automationsMonthModal()).toBeNull();
      await driver.automationsArchiveOpenCustom();
      expect((await driver.automationsMonthModal())?.inputValue).toBe("1");
      expect((await driver.automationsMonthModal())?.error).toBeNull();
      await driver.automationsMonthCancel();
    });

    await test.step("a valid submit forwards to the row and closes", async () => {
      await driver.automationsArchiveOpenCustom();
      await driver.automationsMonthFill("5");
      await driver.automationsMonthSubmit();
      await expect.poll(async () => driver.automationsMonthModal(), { timeout: 30_000 }).toBeNull();
      await expect
        .poll(async () => (await driver.automationsArchiveRow()).pickerLabel, { timeout: 30_000 })
        .toBe("5 months");
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(5);
    });

    await test.step("a fractional value never submits", async () => {
      await driver.automationsArchiveOpenCustom();
      await driver.automationsMonthFill("2.5");
      await driver.automationsMonthSubmit();
      // The number input's native validation blocks the submit: the
      // dialog stays open with the input intact and nothing persists.
      const modal = await driver.automationsMonthModal();
      expect(modal).not.toBeNull();
      expect(modal?.inputValue).toBe("2.5");
      expect(modal?.error).toBeNull();
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).archive_in).toBe(5);
      await driver.automationsMonthCancel();
    });

    await test.step("the close row shares the same dialog", async () => {
      await driver.automationsCloseToggle();
      await expect.poll(async () => (await driver.automationsCloseRow()).toggleOn, { timeout: 30_000 }).toBe(true);
      await driver.automationsCloseOpenCustom();
      const modal = await driver.automationsMonthModal();
      expect(modal?.title).toBe("Customize time range");
      await driver.automationsMonthFill("4");
      await driver.automationsMonthSubmit();
      await expect.poll(async () => driver.automationsMonthModal(), { timeout: 30_000 }).toBeNull();
      await expect
        .poll(async () => (await driver.automationsCloseRow()).pickerLabel, { timeout: 30_000 })
        .toBe("4 months");
      expect((await serverProjectAutomations(workspaceSlug, project.id, ownerSession)).close_in).toBe(4);
    });
  }
);
