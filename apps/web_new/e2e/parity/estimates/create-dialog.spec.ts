// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-323): estimates creation dialog — two-step
// shell with step counter, back, cancel and reset-on-reopen; the OSS
// measurement-family radio; templates versus scratch start; value-set
// arrangement (drag reorder, inline edit, add/remove guards); per-value
// validation blocking submit; and submit storing the new active system.
// Rows: EST-008, EST-009, EST-010, EST-011, EST-012, EST-013.
import { expect, test } from "../fixtures";
import type { ParityDriver } from "../drivers/parity-driver";
import {
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateEstimate,
  serverCreateProjectWithFlags,
  serverEstimateDetail,
  serverListEstimates,
  sessionBrowserCookies,
  signInSessionWithRetry,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** Scratch project owned by one scenario; the server cascades its estimates on delete. */
async function scenarioProject(seed: { workspaceSlug: string; email: string; password: string }, tag: string) {
  const session = await signInSessionWithRetry(seed.email, seed.password);
  const projectId = await serverCreateProjectWithFlags(
    seed.workspaceSlug,
    `${tag} project`,
    parityProjectIdentifier("N323"),
    {},
    session
  );
  return { session, projectId };
}

/** Enter the estimates settings screen pre-authenticated (no UI sign-in). */
async function openSettings(
  driver: ParityDriver,
  seed: { workspaceSlug: string },
  projectId: string,
  session: string
): Promise<void> {
  // Cookie entry, not the UI sign-in flow: faster and immune to the
  // shared box's rate-limit bursts. The dialog rows own no sign-in
  // behavior (that is the shell child's EST-001), so setup skips it.
  await driver.openAuthenticated(
    `/${seed.workspaceSlug}/settings/projects/${projectId}/estimates`,
    sessionBrowserCookies(session)
  );
  await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
}

test(
  specTitle(["EST-008"], "creation dialog walks two steps with back, cancel and reset-on-reopen"),
  { tag: specTags(["EST-008"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 shell ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);

      await test.step("dialog opens on step one with a step counter and no back control", async () => {
        await driver.estimatesOpenCreateDialog();
        expect(await driver.estimatesDialogStep()).toBe("1");
        expect(await driver.estimatesBackVisible()).toBe(false);
      });

      await test.step("picking a starter moves to step two and offers back", async () => {
        await driver.estimatesPickStarter("Custom");
        expect(await driver.estimatesDialogStep()).toBe("2");
        expect(await driver.estimatesBackVisible()).toBe(true);
        expect(await driver.estimatesStepTwoValues()).toEqual(["1", "2"]);
      });

      await test.step("back returns to step one discarding entered values", async () => {
        await driver.estimatesEditValueStart(0, "control");
        await driver.estimatesOpenFormSave("9");
        expect(await driver.estimatesStepTwoValues()).toEqual(["9", "2"]);
        await driver.estimatesDialogBack();
        expect(await driver.estimatesDialogStep()).toBe("1");
        await driver.estimatesPickStarter("Custom");
        expect(await driver.estimatesStepTwoValues()).toEqual(["1", "2"]);
      });

      await test.step("cancel closes with nothing saved server-side", async () => {
        await driver.estimatesDialogCancel();
        expect(await driver.estimatesDialogOpen()).toBe(false);
        expect(await serverListEstimates(seed.workspaceSlug, projectId, session)).toEqual([]);
      });

      await test.step("reopening resets all inputs", async () => {
        await driver.estimatesOpenCreateDialog();
        expect(await driver.estimatesDialogStep()).toBe("1");
        const options = await driver.estimatesFamilyOptions();
        expect(options.map((option) => option.checked)).toEqual([true, false]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-009"], "OSS family radio offers points and categories only"),
  { tag: specTags(["EST-009"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 family ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();

      await test.step("points and categories render enabled with points selected", async () => {
        const options = await driver.estimatesFamilyOptions();
        expect(options.map((option) => option.value)).toEqual(["points", "categories"]);
        expect(options.every((option) => !option.disabled)).toBe(true);
        expect(options.map((option) => option.checked)).toEqual([true, false]);
      });

      await test.step("the time family is absent from the OSS dialog", async () => {
        const options = await driver.estimatesFamilyOptions();
        expect(options.some((option) => option.value === "time")).toBe(false);
      });

      await test.step("both families are selectable and switch the starters", async () => {
        await driver.estimatesSelectFamily("categories");
        const categoriesCards = await driver.estimatesStarterCards();
        expect(categoriesCards.map((card) => card.title)).toContain("T-Shirt Sizes");
        await driver.estimatesSelectFamily("points");
        const pointsCards = await driver.estimatesStarterCards();
        expect(pointsCards.map((card) => card.title)).toContain("Fibonacci");
      });

      await test.step("an OSS submit stores a non-time system", async () => {
        await driver.estimatesPickStarter("Custom");
        await driver.estimatesSubmit();
        expect(await driver.estimatesDialogOpen()).toBe(false);
        const systems = await serverListEstimates(seed.workspaceSlug, projectId, session);
        expect(systems).toHaveLength(1);
        expect(systems[0]?.type).not.toBe("time");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-010"], "template cards preview and load values; scratch starts minimal"),
  { tag: specTags(["EST-010"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 starters ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();

      await test.step("points template cards preview the values they load", async () => {
        const cards = await driver.estimatesStarterCards();
        const fibonacci = cards.find((card) => card.title === "Fibonacci");
        expect(fibonacci?.detail).toContain("1, 2, 3, 5, 8, 13");
        expect(cards.find((card) => card.title === "Linear")?.detail).toContain("1, 2, 3, 4, 5, 6");
        expect(cards.find((card) => card.title === "Squares")?.detail).toContain("1, 4, 9, 16, 25, 36");
      });

      await test.step("picking a template loads its values into step two", async () => {
        await driver.estimatesPickStarter("Fibonacci");
        expect(await driver.estimatesStepTwoValues()).toEqual(["1", "2", "3", "5", "8", "13"]);
        await driver.estimatesDialogBack();
      });

      await test.step("scratch starts from minimal starter values", async () => {
        await driver.estimatesPickStarter("Custom");
        expect(await driver.estimatesStepTwoValues()).toEqual(["1", "2"]);
        await driver.estimatesDialogBack();
      });

      await test.step("categories templates and scratch load their own values", async () => {
        await driver.estimatesSelectFamily("categories");
        const cards = await driver.estimatesStarterCards();
        expect(cards.find((card) => card.title === "T-Shirt Sizes")?.detail).toContain("XS, S, M, L, XL, XXL");
        await driver.estimatesPickStarter("T-Shirt Sizes");
        expect(await driver.estimatesStepTwoValues()).toEqual(["XS", "S", "M", "L", "XL", "XXL"]);
        await driver.estimatesDialogBack();
        await driver.estimatesSelectFamily("categories");
        await driver.estimatesPickStarter("Custom");
        expect(await driver.estimatesStepTwoValues()).toEqual(["Easy", "Hard"]);
      });

      await test.step("browsing starters stores nothing", async () => {
        await driver.estimatesDialogCancel();
        expect(await serverListEstimates(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-011"], "a real drag reorders values with renumbered positions"),
  { tag: specTags(["EST-011"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 drag ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();
      await driver.estimatesPickStarter("Custom");
      await driver.estimatesAddOpen();
      await driver.estimatesOpenFormSave("3");

      await test.step("a real drag reorders the rows", async () => {
        await driver.estimatesDragValueBefore(0, 2);
        expect(await driver.estimatesStepTwoValues()).toEqual(["2", "1", "3"]);
      });

      await test.step("the new order persists with renumbered positions", async () => {
        await driver.estimatesSubmit();
        expect(await driver.estimatesDialogOpen()).toBe(false);
        const systems = await serverListEstimates(seed.workspaceSlug, projectId, session);
        expect(systems).toHaveLength(1);
        const points = [...(systems[0]?.points ?? [])].sort((a, b) => a.key - b.key);
        expect(points.map((point) => point.key)).toEqual([1, 2, 3]);
        expect(points.map((point) => point.value)).toEqual(["2", "1", "3"]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-011"], "value set edits inline and guards add and remove"),
  { tag: specTags(["EST-011"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 guards ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();
      await driver.estimatesPickStarter("Custom");

      await test.step("double-click edits a value inline", async () => {
        await driver.estimatesEditValueStart(0, "double-click");
        expect(await driver.estimatesOpenFormValue()).toBe("1");
        await driver.estimatesOpenFormSave("10");
        expect(await driver.estimatesStepTwoValues()).toEqual(["10", "2"]);
      });

      await test.step("the edit control edits a value inline", async () => {
        await driver.estimatesEditValueStart(1, "control");
        expect(await driver.estimatesOpenFormValue()).toBe("2");
        await driver.estimatesOpenFormSave("20");
        expect(await driver.estimatesStepTwoValues()).toEqual(["10", "20"]);
      });

      await test.step("remove hides at two values and shows above", async () => {
        expect(await driver.estimatesRemoveVisible(0)).toBe(false);
        expect(await driver.estimatesRemoveVisible(1)).toBe(false);
        await driver.estimatesAddOpen();
        await driver.estimatesOpenFormSave("3");
        expect(await driver.estimatesRemoveVisible(0)).toBe(true);
        await driver.estimatesRemoveValue(0);
        expect(await driver.estimatesStepTwoValues()).toEqual(["20", "3"]);
        expect(await driver.estimatesRemoveVisible(0)).toBe(false);
      });

      await test.step("add hides at six values", async () => {
        expect(await driver.estimatesAddVisible()).toBe(true);
        for (const value of ["4", "5", "6", "7"]) {
          await driver.estimatesAddOpen();
          await driver.estimatesOpenFormSave(value);
        }
        expect(await driver.estimatesStepTwoValues()).toEqual(["20", "3", "4", "5", "6", "7"]);
        expect(await driver.estimatesAddVisible()).toBe(false);
      });

      await test.step("arranging without submit stores nothing", async () => {
        await driver.estimatesDialogCancel();
        expect(await serverListEstimates(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-012"], "per-value validation refuses bad rows and blocks submit"),
  { tag: specTags(["EST-012"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 validation ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();
      await driver.estimatesPickStarter("Custom");

      await test.step("numeric families refuse non-numbers", async () => {
        await driver.estimatesAddOpen();
        await driver.estimatesOpenFormFill("abc");
        await driver.estimatesOpenFormSubmit();
        expect(await driver.estimatesOpenFormCount()).toBe(1);
        expect(await driver.estimatesValueErrorCount()).toBe(1);
        expect((await driver.estimatesValueErrors()).join(" ")).toMatch(/numeric/i);
        await driver.estimatesOpenFormCancel();
      });

      await test.step("numeric families refuse zero", async () => {
        await driver.estimatesAddOpen();
        await driver.estimatesOpenFormFill("0");
        await driver.estimatesOpenFormSubmit();
        expect((await driver.estimatesValueErrors()).join(" ")).toMatch(/greater than 0/i);
        await driver.estimatesOpenFormCancel();
      });

      await test.step("duplicates are refused", async () => {
        await driver.estimatesAddOpen();
        await driver.estimatesOpenFormFill("1");
        await driver.estimatesOpenFormSubmit();
        expect((await driver.estimatesValueErrors()).join(" ")).toMatch(/already exists/i);
        await driver.estimatesOpenFormCancel();
      });

      await test.step("the text family refuses numeric text", async () => {
        await driver.estimatesDialogBack();
        await driver.estimatesSelectFamily("categories");
        await driver.estimatesPickStarter("Custom");
        await driver.estimatesAddOpen();
        await driver.estimatesOpenFormFill("123");
        await driver.estimatesOpenFormSubmit();
        expect((await driver.estimatesValueErrors()).join(" ")).toMatch(/character/i);
        await driver.estimatesOpenFormCancel();
      });

      await test.step("over-long input is capped while typing", async () => {
        await driver.estimatesAddOpen();
        await driver.estimatesAddType("1234567890123456789012345");
        expect((await driver.estimatesOpenFormValue())?.length).toBe(20);
        await driver.estimatesOpenFormCancel();
      });

      await test.step("submit with an unresolved row prompts instead of calling the API", async () => {
        await driver.estimatesAddOpen();
        await driver.estimatesSubmit();
        expect(await driver.estimatesDialogOpen()).toBe(true);
        expect(await driver.estimatesValueErrorCount()).toBe(1);
        expect((await driver.estimatesValueErrors()).join(" ")).toMatch(/empty/i);
        expect(await serverListEstimates(seed.workspaceSlug, projectId, session)).toEqual([]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-013"], "valid submit stores the active system"),
  { tag: specTags(["EST-013"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 submit ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();
      await driver.estimatesPickStarter("Custom");
      await driver.estimatesAddOpen();
      await driver.estimatesOpenFormSave("3");

      await driver.estimatesSubmit();

      await test.step("the dialog closes with a success notice", async () => {
        expect(await driver.estimatesDialogOpen()).toBe(false);
        await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/estimate created/i);
      });

      await test.step("the system is stored as the project's in-use system", async () => {
        const systems = await serverListEstimates(seed.workspaceSlug, projectId, session);
        expect(systems).toHaveLength(1);
        expect(systems[0]?.name).toBe("Points");
        expect(systems[0]?.type).toBe("points");
        expect(systems[0]?.last_used).toBe(true);
        const detail = await serverEstimateDetail(seed.workspaceSlug, projectId, systems[0]?.id ?? "", session);
        const points = [...detail.points].sort((a, b) => a.key - b.key);
        expect(points.map((point) => point.value)).toEqual(["1", "2", "3"]);
      });

      await test.step("the screen shows the new system as active", async () => {
        await expect
          .poll(() => driver.estimatesActiveRow(), { timeout: 15_000 })
          .toEqual({ name: "Points", values: "1, 2, 3" });
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-013"], "failed submit reports an error and preserves the dialog"),
  { tag: specTags(["EST-013"]) },
  async ({ driver, seed }) => {
    const tag = `NF323 submitfail ${Date.now()}`;
    const { session, projectId } = await scenarioProject(seed, tag);
    // Same-name inactive system: the empty state still offers the dialog,
    // but the server refuses the duplicate name on submit.
    await serverCreateEstimate(
      seed.workspaceSlug,
      projectId,
      "Points",
      ["9", "10"],
      session,
      "points",
      undefined,
      false
    );
    try {
      await openSettings(driver, seed, projectId, session);
      await driver.estimatesOpenCreateDialog();
      await driver.estimatesPickStarter("Custom");

      await driver.estimatesSubmit();

      await test.step("an error is reported", async () => {
        await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/creation failed/i);
      });

      await test.step("the dialog stays open with its content preserved", async () => {
        expect(await driver.estimatesDialogOpen()).toBe(true);
        expect(await driver.estimatesDialogStep()).toBe("2");
        expect(await driver.estimatesStepTwoValues()).toEqual(["1", "2"]);
      });

      await test.step("nothing new is stored server-side", async () => {
        const systems = await serverListEstimates(seed.workspaceSlug, projectId, session);
        expect(systems).toHaveLength(1);
        expect(systems[0]?.points.map((point) => point.value).sort()).toEqual(["10", "9"]);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
