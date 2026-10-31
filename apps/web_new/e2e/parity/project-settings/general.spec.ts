// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-389): project settings General tab view and
// edit/save — the tab renders every field with current values behind a
// skeleton, admins save through the project update endpoint with
// per-field rules and conflict errors, and non-admins get the same tab
// dimmed and inert with no danger zone. Rows: PSET-005 (view the General
// tab), PSET-006 (edit and save as admin; read-only otherwise).
//
// Observed behavior notes: the banner carries a cover image plus the emblem
// and a name/identifier headline; name is required at <=255 chars;
// identifier strips to uppercase alphanumerics, requires 1-10 chars, and is
// prechecked for availability only when it changed; the precheck consults
// the identifier reservation table (one row per created project, and the
// endpoint answers a COUNT, not a boolean), so a reserved identifier stops
// the save with a field error while the reservation-less seeded identifier
// sails through and collides at write time with its own already-taken
// toast; base branch caps at
// 128 chars of [A-Za-z0-9._/-]; repo URL caps at 512 and never rides the
// normal save. TARGET (main#523, OPEN): the cover banner is removed
// entirely — the cover assertions below pin current behavior and must move
// to absence assertions once it merges. TARGET (main#503, OPEN): the
// description placeholder becomes the AI-context text — same treatment.
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  parityProjectIdentifier,
  serverCleanupProject,
  serverCreateProjectWithFlags,
  serverIdentifierTaken,
  serverProjectRecord,
  signInAuthedSession,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** Preset cadence labels as the picker renders them. */
const CADENCE_LABELS: Record<number, string> = {
  1800: "30 minutes",
  3600: "1 hour",
  10800: "3 hours",
  21600: "6 hours",
  43200: "12 hours",
  86400: "24 hours",
};

test(
  specTitle(["PSET-005"], "general tab shows a skeleton first, then every field with current values"),
  { tag: specTags(["PSET-005"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const record = await serverProjectRecord(seed.workspaceSlug, seed.projectId, session);

    await test.step("a stalled details response holds the skeleton before the form", async () => {
      // Cold entry: cookies in, straight to the tab, so the store starts
      // empty; aborting the details fetches pins the tab on its loader
      // instead of racing it, and releasing lets the form resolve.
      const authed = await signInAuthedSession(seed.email, seed.password);
      await driver.settingsGeneralStallDetails();
      await driver.openAuthenticated(
        `/${seed.workspaceSlug}/settings/projects/${seed.projectId}`,
        browserSessionCookies(authed)
      );
      await expect.poll(() => driver.settingsGeneralSkeletonVisible(), { timeout: 30_000 }).toBe(true);
      await driver.settingsGeneralReleaseStall();
      await expect.poll(() => driver.settingsGeneralFormVisible(), { timeout: 60_000 }).toBe(true);
    });

    await test.step("banner, headline and creation stamp render", async () => {
      // The seeded project carries no cover artwork, so the banner image
      // itself is proven in artwork.spec (a gallery pick renders it);
      // TARGET (main#523): no cover banner renders anywhere on the tab.
      const header = await driver.settingsGeneralHeaderText();
      expect(header).toContain(String(record["name"]));
      expect(header).toContain(String(record["identifier"]));
      const created = await driver.settingsGeneralCreatedText();
      expect(created).toMatch(/^Created on\s+\S/);
    });

    if (driver.target === "web_new") {
      // TARGET (main#503/main#523, NEWFRONT-606): the tab shows the
      // AI-context placeholder and renders no cover banner anywhere.
      await test.step("the tab carries the AI-context placeholder and no cover banner", async () => {
        expect(await driver.settingsGeneralDescriptionPlaceholder()).toBe(
          "Project Context (Will be shown to AI agents)"
        );
        expect(await driver.settingsGeneralCoverVisible()).toBe(false);
      });
    }

    await test.step("text fields match the server record", async () => {
      expect(await driver.settingsGeneralFieldValue("name")).toBe(String(record["name"] ?? ""));
      expect(await driver.settingsGeneralFieldValue("description")).toBe(String(record["description"] ?? ""));
      expect(await driver.settingsGeneralFieldValue("identifier")).toBe(String(record["identifier"] ?? ""));
      expect(await driver.settingsGeneralFieldValue("repoUrl")).toBe(String(record["repo_url"] ?? ""));
      expect(await driver.settingsGeneralFieldValue("baseBranch")).toBe(String(record["base_branch"] ?? ""));
    });

    await test.step("network, timezone and execution defaults match the server record", async () => {
      // Visibility network renders by numeric key (0 Private, 2 Public).
      const networkLabels: Record<number, string> = { 0: "Private", 2: "Public" };
      const networkLabel = networkLabels[Number(record["network"] ?? 0)] ?? "";
      expect(networkLabel).not.toBe("");
      expect(await driver.settingsGeneralNetworkText()).toContain(networkLabel);
      expect(await driver.settingsGeneralTimezoneText()).toContain(String(record["timezone"] ?? ""));
      expect(await driver.settingsGeneralExecutorValue()).toBe(
        String(record["default_agent_executor"] ?? "local_runner")
      );
      expect(await driver.settingsGeneralBudgetValue()).toBe(String(record["agent_default_max_ticks"] ?? ""));
      const cadenceSeconds = Number(record["agent_default_interval_seconds"] ?? 10800);
      const cadenceText = await driver.settingsGeneralCadenceText();
      if (CADENCE_LABELS[cadenceSeconds] !== undefined) {
        expect(cadenceText).toBe(CADENCE_LABELS[cadenceSeconds]);
      } else {
        expect(cadenceText).toContain("Custom");
      }
    });
  }
);

test(
  specTitle(["PSET-006"], "admin edits name and description, saves, and the server persists it"),
  { tag: specTags(["PSET-006"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 edit ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    const renamed = `${tag} renamed`;
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);

      await test.step("a valid rename persists and confirms", async () => {
        await driver.settingsGeneralFill("name", renamed);
        await driver.settingsGeneralFill("description", `${tag} description`);
        await driver.settingsGeneralSave();
        const toast = await driver.rulesLastToast();
        expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`).toContain("Project updated successfully");
        await expect
          .poll(async () => String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["name"]), {
            timeout: 30_000,
          })
          .toBe(renamed);
      });

      await test.step("the banner headline follows the rename", async () => {
        await expect.poll(() => driver.settingsGeneralHeaderText(), { timeout: 30_000 }).toContain(renamed);
      });

      await test.step("the sidebar row follows the rename", async () => {
        await driver.openWorkspacePath(`/${seed.workspaceSlug}/projects`);
        await expect.poll(() => driver.projectRowHref(renamed), { timeout: 30_000 }).not.toBeNull();
        await expect.poll(() => driver.projectRowHref(`${tag} project`), { timeout: 30_000 }).toBeNull();
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-006"], "required fields, character rules and conflicts refuse with field errors"),
  { tag: specTags(["PSET-006"]) },
  async ({ driver, seed }) => {
    const tag = `NF389 rules ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N389"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.settingsGeneralOpen(seed.workspaceSlug, projectId);
      const before = await serverProjectRecord(seed.workspaceSlug, projectId, session);

      await test.step("a blank name blocks the save with a field error", async () => {
        await driver.settingsGeneralFill("name", "");
        await driver.settingsGeneralSave();
        expect(await driver.settingsGeneralFieldError("name")).not.toBe("");
        expect(String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["name"])).toBe(
          String(before["name"])
        );
      });

      await test.step("identifier input strips and uppercases as typed", async () => {
        await driver.settingsGeneralFill("identifier", "ab-12");
        expect(await driver.settingsGeneralFieldValue("identifier")).toBe("AB12");
      });

      await test.step("an over-long identifier blocks the save with a field error", async () => {
        await driver.settingsGeneralFill("identifier", "ABCDEFGHIJK");
        await driver.settingsGeneralSave();
        expect(await driver.settingsGeneralFieldError("identifier")).not.toBe("");
      });

      await test.step("a reserved identifier stops the save with a field error", async () => {
        // Project creation reserves the identifier, so a sibling
        // throwaway's identifier trips the live availability precheck.
        const siblingId = await serverCreateProjectWithFlags(
          seed.workspaceSlug,
          `${tag} sibling`,
          parityProjectIdentifier("N389"),
          {},
          session
        );
        try {
          const sibling = await serverProjectRecord(seed.workspaceSlug, siblingId, session);
          expect(await serverIdentifierTaken(seed.workspaceSlug, String(sibling["identifier"]), session)).toBe(true);
          await driver.settingsGeneralFill("name", `${tag} project`);
          await driver.settingsGeneralFill("identifier", String(sibling["identifier"]));
          await driver.settingsGeneralSave();
          expect(await driver.settingsGeneralFieldError("identifier")).not.toBe("");
          expect(String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["identifier"])).toBe(
            String(before["identifier"])
          );
        } finally {
          await serverCleanupProject(seed.workspaceSlug, siblingId, session);
        }
      });

      await test.step("an unreserved live identifier collides at write with its own notice", async () => {
        // The seeded project carries no reservation row, so the precheck
        // waves its identifier through and the write refuses it instead.
        // The target (PSET-006, inventory) reports the refusal on the
        // field with no toast; the old app toasts and leaves the field
        // clean, so the oracle branch keeps its observation.
        const seeded = await serverProjectRecord(seed.workspaceSlug, seed.projectId, session);
        expect(await serverIdentifierTaken(seed.workspaceSlug, String(seeded["identifier"]), session)).toBe(false);
        await driver.settingsGeneralFill("name", `${tag} project`);
        await driver.settingsGeneralFill("identifier", String(seeded["identifier"]));
        await driver.settingsGeneralSave();
        if (driver.target === "web_new") {
          expect(await driver.settingsGeneralFieldError("identifier")).toContain("already taken");
        } else {
          expect(await driver.settingsGeneralFieldError("identifier")).toBe("");
          const toast = await driver.rulesLastToast();
          expect(`${toast?.title ?? ""} ${toast?.message ?? ""}`).toContain("already taken");
        }
        expect(String((await serverProjectRecord(seed.workspaceSlug, projectId, session))["identifier"])).toBe(
          String(before["identifier"])
        );
      });

      await test.step("a bad base branch blocks the save with a field error", async () => {
        await driver.settingsGeneralFill("identifier", String(before["identifier"]));
        await driver.settingsGeneralFill("baseBranch", "no spaces allowed!");
        await driver.settingsGeneralSave();
        expect(await driver.settingsGeneralFieldError("baseBranch")).not.toBe("");
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["PSET-006"], "non-admins see the tab dimmed and inert with no danger zone"),
  { tag: specTags(["PSET-006"]) },
  async ({ driver, seed }) => {
    if (seed.guestEmail === undefined || seed.guestPassword === undefined) {
      throw new Error("[parity] the seed carries no guest identity for the read-only scenario.");
    }
    await driver.rulesEnsureSignedIn(seed.guestEmail, seed.guestPassword, seed.workspaceSlug);
    await driver.settingsGeneralOpen(seed.workspaceSlug, seed.projectId);

    expect(await driver.settingsGeneralFormVisible()).toBe(true);
    expect(await driver.settingsGeneralFormDimmed()).toBe(true);
    for (const field of ["name", "description", "identifier", "repoUrl", "baseBranch"] as const) {
      expect(await driver.settingsGeneralFieldDisabled(field)).toBe(true);
    }
    expect(await driver.settingsGeneralDangerVisible()).toBe(false);
  }
);
