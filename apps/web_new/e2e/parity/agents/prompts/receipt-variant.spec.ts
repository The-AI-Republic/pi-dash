// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-186): an expanded receipt contrasts the
// personal assembly with the workspace-only variant automatic runs use,
// but only while the viewer holds a personal override; without overrides
// a single block shows.
// Row: AGT-029.
import { test, expect } from "../../fixtures";
import { serverPromptCompiled, serverPromptSectionRevert, serverPromptSectionUpsert } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatMember } from "../support";
import { EDITABLE_SECTION } from "./support";

const ROWS = ["AGT-029"];
const PERSONAL_BODY = "Parity AGT-029 personal override marker.";

test(
  specTitle(ROWS, "receipt contrasts the personal template with the automatic-run variant"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace and member", async () =>
      schedulerHarness("parity-agt29"));
    const { ownerSession, workspaceSlug } = harness;
    const member = await seatMember(harness);

    await test.step("member holds a personal override", async () => {
      await serverPromptSectionUpsert(workspaceSlug, EDITABLE_SECTION, "user", PERSONAL_BODY, member.session);
    });

    const server = await test.step("server compiles both variants", async () => {
      const personal = await serverPromptCompiled(workspaceSlug, "coding-task", "user", member.session);
      expect(personal.template_body).toContain(PERSONAL_BODY);
      if (personal.automatic_template_body === undefined) {
        throw new Error("[parity] expected the automatic-run variant.");
      }
      expect(personal.automatic_template_body).not.toContain(PERSONAL_BODY);
      const plain = await serverPromptCompiled(workspaceSlug, "coding-task", "user", ownerSession);
      expect(plain.automatic_template_body).toBeUndefined();
      return { personal, plain };
    });

    await test.step("member signs in and expands the coding-task receipt", async () => {
      await driver.rulesEnsureSignedIn(member.email, member.password, workspaceSlug);
      await driver.promptsOpen(workspaceSlug);
      await driver.promptsOpenTab("Receipt");
      await expect.poll(async () => (await driver.promptsReceiptCards()).length, { timeout: 30_000 }).toBe(3);
      await driver.promptsReceiptToggle("coding-task");
      expect(await driver.promptsReceiptExpanded("coding-task")).toBe(true);
    });

    await test.step("both the personal and the automatic assemblies render", async () => {
      expect(await driver.promptsReceiptTemplate("coding-task")).toBe(server.personal.template_body.trimEnd());
      expect(await driver.promptsReceiptAutomatic("coding-task")).toBe(
        (server.personal.automatic_template_body ?? "").trimEnd()
      );
    });

    await test.step("without overrides only one block shows", async () => {
      await serverPromptSectionRevert(workspaceSlug, EDITABLE_SECTION, "user", member.session);
      await driver.promptsOpen(workspaceSlug);
      await driver.promptsOpenTab("Receipt");
      await expect.poll(async () => (await driver.promptsReceiptCards()).length, { timeout: 30_000 }).toBe(3);
      await driver.promptsReceiptToggle("coding-task");
      expect(await driver.promptsReceiptTemplate("coding-task")).toBe(server.plain.template_body.trimEnd());
      expect(await driver.promptsReceiptAutomatic("coding-task")).toBeNull();
    });
  }
);
