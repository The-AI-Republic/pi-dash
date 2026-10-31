// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-326): the estimates permissions matrix
// (EST-024) — every endpoint answered per role, refused server-side plus
// gated in the UI — and the edition split (EST-029): the OSS
// points-and-categories subset boundary with whole-system deletion, and
// the desktop entry-point pin.
// Rows: EST-024, EST-029.
//
// One browser role per test: the shared sign-in helper keeps an existing
// session, so switching users mid-test would silently keep the first
// session. Fresh contexts per test give each role a clean sign-in
// instead. The server-side matrix needs no browser at all — it probes
// the endpoints with one session cookie per role.
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverAddProjectMembers,
  serverCleanupProject,
  serverCleanupWorkspaceMember,
  serverCompleteOnboarding,
  serverCreateEstimate,
  serverCreateProjectWithFlags,
  serverEstimateRaw,
  serverEstimatesList,
  serverProjectMembers,
  serverProvisionWorkspaceMember,
  serverSetProjectEstimate,
  serverWorkspaceMembers,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

function requireGuest(seed: ParitySeedFacts): { email: string; password: string } {
  if (!seed.guestEmail || !seed.guestPassword)
    throw new Error("[parity] seed has no guest identity; rerun parity-up.sh from a clean checkout.");
  return { email: seed.guestEmail, password: seed.guestPassword };
}

/** Seed guest user id, resolved live (never hardcoded across stacks). */
async function seedGuestUserId(seed: ParitySeedFacts, session: string): Promise<string> {
  const members = await serverProjectMembers(seed.workspaceSlug, seed.projectId, session);
  const guest = members.find((row) => row.role === 5);
  if (!guest) throw new Error("[parity] seed project has no role-5 guest member.");
  return guest.userId;
}

/** Remove a provisioned workspace member; lenient for finally-blocks. */
async function cleanupProvisionedMember(seed: ParitySeedFacts, userId: string, session: string): Promise<void> {
  const membership = await serverWorkspaceMembers(seed.workspaceSlug, session).catch(() => []);
  const mine = membership.find((row) => row.userId === userId);
  if (mine) await serverCleanupWorkspaceMember(seed.workspaceSlug, mine.membershipId, session);
}

function estimatesBase(workspaceSlug: string, projectId: string): string {
  return `/api/workspaces/${workspaceSlug}/projects/${projectId}/estimates/`;
}

function systemPath(workspaceSlug: string, projectId: string, estimateId: string): string {
  return `${estimatesBase(workspaceSlug, projectId)}${estimateId}/`;
}

function pointPath(workspaceSlug: string, projectId: string, estimateId: string, pointId: string): string {
  return `${systemPath(workspaceSlug, projectId, estimateId)}estimate-points/${pointId}/`;
}

function pointsPath(workspaceSlug: string, projectId: string, estimateId: string): string {
  return `${systemPath(workspaceSlug, projectId, estimateId)}estimate-points/`;
}

function projectPointsPath(workspaceSlug: string, projectId: string): string {
  return `/api/workspaces/${workspaceSlug}/projects/${projectId}/project-estimates/`;
}

test(
  specTitle(["EST-024"], "estimate endpoints answer per role and refuse the rest server-side"),
  { tag: specTags(["EST-024"]) },
  async ({ seed }) => {
    const tag = `NF326 matrix ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guestId = await seedGuestUserId(seed, session);
    const guestSession = await signInSession(guestCreds.email, guestCreds.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf326matrixm", session);
    await serverCompleteOnboarding(member.session);
    const outsider = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf326matrixo", session);
    await serverCompleteOnboarding(outsider.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    await serverAddProjectMembers(
      seed.workspaceSlug,
      projectId,
      [
        { memberId: member.userId, role: 15 },
        { memberId: guestId, role: 5 },
      ],
      session
    );
    // The outsider stays a workspace member with no project membership:
    // every project-scoped read must refuse them.
    const reader = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} reader`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, reader.id, session);
    try {
      const roles = await serverProjectMembers(seed.workspaceSlug, projectId, session);
      expect(roles.find((row) => row.userId === member.userId)?.role).toBe(15);
      expect(roles.find((row) => row.userId === guestId)?.role).toBe(5);
      expect(roles.find((row) => row.userId === outsider.userId)).toBeUndefined();

      await test.step("reads need project membership: admin, member and guest read; outsiders do not", async () => {
        for (const [who, cookie] of [
          ["admin", session],
          ["member", member.session],
          ["guest", guestSession],
        ] as const) {
          const list = await serverEstimateRaw("GET", estimatesBase(seed.workspaceSlug, projectId), cookie);
          expect(list.status, `${who} list`).toBe(200);
          const single = await serverEstimateRaw("GET", systemPath(seed.workspaceSlug, projectId, reader.id), cookie);
          expect(single.status, `${who} single`).toBe(200);
        }
        const guestList = await serverEstimateRaw("GET", estimatesBase(seed.workspaceSlug, projectId), guestSession);
        expect((guestList.json as { id?: string }[]).map((row) => row.id)).toContain(reader.id);
        expect(
          (await serverEstimateRaw("GET", estimatesBase(seed.workspaceSlug, projectId), outsider.session)).status
        ).toBe(403);
        expect(
          (await serverEstimateRaw("GET", systemPath(seed.workspaceSlug, projectId, reader.id), outsider.session))
            .status
        ).toBe(403);
        expect((await serverEstimateRaw("GET", estimatesBase(seed.workspaceSlug, projectId), "")).status).toBe(401);
      });

      await test.step("the project-points read needs admin or member; guests and outsiders are refused", async () => {
        const path = projectPointsPath(seed.workspaceSlug, projectId);
        expect((await serverEstimateRaw("GET", path, session)).status).toBe(200);
        expect((await serverEstimateRaw("GET", path, member.session)).status).toBe(200);
        const adminPoints = await serverEstimateRaw("GET", path, session);
        expect((adminPoints.json as { value?: string }[]).map((row) => row.value).sort()).toEqual(["1", "2"]);
        expect((await serverEstimateRaw("GET", path, guestSession)).status).toBe(403);
        expect((await serverEstimateRaw("GET", path, outsider.session)).status).toBe(403);
      });

      await test.step("system creation needs admin or member; refused calls store nothing", async () => {
        const before = await serverEstimatesList(seed.workspaceSlug, projectId, session);
        const payload = (name: string) => ({
          estimate: { name, type: "points", last_used: false },
          estimate_points: [{ value: "1" }, { value: "2" }],
        });
        expect(
          (
            await serverEstimateRaw(
              "POST",
              estimatesBase(seed.workspaceSlug, projectId),
              member.session,
              payload(`${tag} member-made`)
            )
          ).status
        ).toBe(200);
        expect(
          (
            await serverEstimateRaw(
              "POST",
              estimatesBase(seed.workspaceSlug, projectId),
              guestSession,
              payload(`${tag} guest-made`)
            )
          ).status
        ).toBe(403);
        expect(
          (
            await serverEstimateRaw(
              "POST",
              estimatesBase(seed.workspaceSlug, projectId),
              outsider.session,
              payload(`${tag} outsider-made`)
            )
          ).status
        ).toBe(403);
        expect(
          (
            await serverEstimateRaw(
              "POST",
              estimatesBase(seed.workspaceSlug, projectId),
              "",
              payload(`${tag} anon-made`)
            )
          ).status
        ).toBe(401);
        const after = await serverEstimatesList(seed.workspaceSlug, projectId, session);
        expect(after).toHaveLength(before.length + 1);
        expect(after.map((row) => row.name)).toContain(`${tag} member-made`);
      });

      await test.step("system updates need admin or member; refused calls change nothing", async () => {
        const pointId = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find(
          (row) => row.id === reader.id
        )?.points[0]?.id;
        expect(pointId).not.toBeUndefined();
        const payload = (name: string) => ({
          estimate: { name },
          estimate_points: [{ id: pointId, value: "7" }],
        });
        const path = systemPath(seed.workspaceSlug, projectId, reader.id);
        expect((await serverEstimateRaw("PATCH", path, guestSession, payload(`${tag} guest-renamed`))).status).toBe(
          403
        );
        expect(
          (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find((row) => row.id === reader.id)?.name
        ).toBe(`${tag} reader`);
        expect((await serverEstimateRaw("PATCH", path, member.session, payload(`${tag} member-renamed`))).status).toBe(
          200
        );
        expect(
          (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find((row) => row.id === reader.id)?.name
        ).toBe(`${tag} member-renamed`);
        expect((await serverEstimateRaw("PATCH", path, session, payload(`${tag} admin-renamed`))).status).toBe(200);
        const renamed = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find(
          (row) => row.id === reader.id
        );
        expect(renamed?.name).toBe(`${tag} admin-renamed`);
        expect(renamed?.points.map((point) => point.value).sort()).toEqual(["2", "7"]);
      });

      await test.step("point writes need admin or member; refused calls store and change nothing", async () => {
        const collection = pointsPath(seed.workspaceSlug, projectId, reader.id);
        const adminMade = await serverEstimateRaw("POST", collection, session, { key: 11, value: "11" });
        expect(adminMade.status).toBe(200);
        const memberMade = await serverEstimateRaw("POST", collection, member.session, { key: 12, value: "12" });
        expect(memberMade.status).toBe(200);
        expect((await serverEstimateRaw("POST", collection, guestSession, { key: 13, value: "13" })).status).toBe(403);
        expect((await serverEstimateRaw("POST", collection, outsider.session, { key: 14, value: "14" })).status).toBe(
          403
        );
        const adminPointId = (adminMade.json as { id?: string }).id;
        const memberPointId = (memberMade.json as { id?: string }).id;
        expect(adminPointId).not.toBeUndefined();
        expect(memberPointId).not.toBeUndefined();
        expect(
          (
            await serverEstimateRaw(
              "PATCH",
              pointPath(seed.workspaceSlug, projectId, reader.id, memberPointId ?? ""),
              member.session,
              { value: "13" }
            )
          ).status
        ).toBe(200);
        expect(
          (
            await serverEstimateRaw(
              "PATCH",
              pointPath(seed.workspaceSlug, projectId, reader.id, adminPointId ?? ""),
              guestSession,
              { value: "99" }
            )
          ).status
        ).toBe(403);
        const values = (await serverEstimatesList(seed.workspaceSlug, projectId, session))
          .find((row) => row.id === reader.id)
          ?.points.map((point) => point.value);
        expect(values).toContain("13");
        expect(values).not.toContain("99");
        expect(
          (
            await serverEstimateRaw(
              "DELETE",
              pointPath(seed.workspaceSlug, projectId, reader.id, memberPointId ?? ""),
              member.session
            )
          ).status
        ).toBe(200);
        expect(
          (
            await serverEstimateRaw(
              "DELETE",
              pointPath(seed.workspaceSlug, projectId, reader.id, adminPointId ?? ""),
              guestSession
            )
          ).status
        ).toBe(403);
        expect(
          (
            await serverEstimateRaw(
              "DELETE",
              pointPath(seed.workspaceSlug, projectId, reader.id, adminPointId ?? ""),
              session
            )
          ).status
        ).toBe(200);
        const remaining = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find(
          (row) => row.id === reader.id
        )?.points;
        expect(remaining?.map((point) => point.id)).not.toContain(memberPointId);
        expect(remaining?.map((point) => point.id)).not.toContain(adminPointId);
      });

      await test.step("whole-system deletion needs admin or member; refused calls keep the system", async () => {
        const doomedMember = await serverCreateEstimate(
          seed.workspaceSlug,
          projectId,
          `${tag} doomed-m`,
          ["1"],
          session
        );
        const doomedGuest = await serverCreateEstimate(
          seed.workspaceSlug,
          projectId,
          `${tag} doomed-g`,
          ["1"],
          session
        );
        const doomedAdmin = await serverCreateEstimate(
          seed.workspaceSlug,
          projectId,
          `${tag} doomed-a`,
          ["1"],
          session
        );
        expect(
          (
            await serverEstimateRaw(
              "DELETE",
              systemPath(seed.workspaceSlug, projectId, doomedMember.id),
              member.session
            )
          ).status
        ).toBe(204);
        expect(
          (await serverEstimateRaw("DELETE", systemPath(seed.workspaceSlug, projectId, doomedGuest.id), guestSession))
            .status
        ).toBe(403);
        expect(
          (
            await serverEstimateRaw(
              "DELETE",
              systemPath(seed.workspaceSlug, projectId, doomedGuest.id),
              outsider.session
            )
          ).status
        ).toBe(403);
        expect(
          (await serverEstimateRaw("DELETE", systemPath(seed.workspaceSlug, projectId, doomedAdmin.id), session)).status
        ).toBe(204);
        const names = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.name);
        expect(names).not.toContain(`${tag} doomed-m`);
        expect(names).toContain(`${tag} doomed-g`);
        expect(names).not.toContain(`${tag} doomed-a`);
        expect(names).toContain(`${tag} admin-renamed`);
      });
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await cleanupProvisionedMember(seed, member.userId, session);
      await cleanupProvisionedMember(seed, outsider.userId, session);
    }
  }
);

test(
  specTitle(["EST-024"], "project admins get the working screen, switch and row delete"),
  { tag: specTags(["EST-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF326 admin ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      const listed = await serverEstimatesList(seed.workspaceSlug, projectId, session);
      expect(listed.find((row) => row.id === system.id)?.name).toBe(`${tag} prime`);
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(false);
      expect(await driver.estimatesHeadingVisible()).toBe(true);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(true);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
      // OSS ships delete-only row buttons; the cloud edit action is absent (EST-029 pins that split).
      expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-024"], "members get the denial view with no switch, rows or actions"),
  { tag: specTags(["EST-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF326 member ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "nf326member", session);
    await serverCompleteOnboarding(member.session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: member.userId, role: 15 }], session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      expect(
        (await serverProjectMembers(seed.workspaceSlug, projectId, session)).find((row) => row.userId === member.userId)
          ?.role
      ).toBe(15);
      // Members can write through the API (see the matrix above) but the
      // screen itself is admin-only, so no control is reachable.
      await driver.rulesEnsureSignedIn(member.email, member.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesHeadingVisible()).toBe(false);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(false);
      expect(await driver.estimatesActiveRowButtonCount()).toBe(0);
      expect(system.id).not.toBe("");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
      await cleanupProvisionedMember(seed, member.userId, session);
    }
  }
);

test(
  specTitle(["EST-024"], "guests get the denial view with no switch, rows or actions"),
  { tag: specTags(["EST-024"]) },
  async ({ driver, seed }) => {
    const tag = `NF326 guest ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const guestCreds = requireGuest(seed);
    const guestId = await seedGuestUserId(seed, session);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    await serverAddProjectMembers(seed.workspaceSlug, projectId, [{ memberId: guestId, role: 5 }], session);
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      expect(
        (await serverProjectMembers(seed.workspaceSlug, projectId, session)).find((row) => row.userId === guestId)?.role
      ).toBe(5);
      await driver.rulesEnsureSignedIn(guestCreds.email, guestCreds.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(true);
      expect(await driver.estimatesHeadingVisible()).toBe(false);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(false);
      expect(await driver.estimatesActiveRowButtonCount()).toBe(0);
      expect(system.id).not.toBe("");
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-029"], "the OSS dialog offers exactly points and categories with no time family"),
  { tag: specTags(["EST-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF326 subset ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await driver.estimatesOpenCreateDialog();
      // The edition boundary as a set: dialog mechanics per family are
      // the create sibling's (EST-009); this pins the exact OSS offer.
      const options = await driver.estimatesFamilyOptions();
      expect(options.map((option) => option.value)).toEqual(["points", "categories"]);
      expect(options.every((option) => !option.disabled)).toBe(true);
      expect(options.some((option) => option.value === "time")).toBe(false);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-029"], "points and categories systems create and delete through the OSS screen"),
  { tag: specTags(["EST-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF326 smoke ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      const starters: { family: "points" | "categories"; values: string[] }[] = [
        { family: "points", values: ["1", "2"] },
        { family: "categories", values: ["Easy", "Hard"] },
      ];
      for (const starter of starters) {
        await test.step(`a ${starter.family} system creates through the dialog`, async () => {
          const before = await serverEstimatesList(seed.workspaceSlug, projectId, session);
          await driver.estimatesOpenCreateDialog();
          if (starter.family === "points") {
            // Points starts selected (EST-009 pins the default); clicking
            // its label now would also match the Custom starter card, so
            // the precondition is asserted, not clicked.
            const options = await driver.estimatesFamilyOptions();
            expect(options.find((option) => option.value === "points")?.checked).toBe(true);
          } else {
            await driver.estimatesSelectFamily(starter.family);
          }
          await driver.estimatesPickStarter("Custom");
          await driver.estimatesSubmit();
          expect(await driver.estimatesDialogOpen()).toBe(false);
          const created = (await serverEstimatesList(seed.workspaceSlug, projectId, session)).find(
            (row) => !before.some((old) => old.id === row.id)
          );
          expect(created).not.toBeUndefined();
          expect(created?.points.map((point) => point.value).sort()).toEqual([...starter.values].sort());
          // Stored as the in-use system (the project link itself is the
          // enable switch's write — EST-004 — not the dialog's).
          expect(created?.lastUsed).toBe(true);
          await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(created?.name ?? "");
        });

        await test.step(`the ${starter.family} system deletes through its row action`, async () => {
          const victim = (await serverEstimatesList(seed.workspaceSlug, projectId, session))[0];
          expect(victim).not.toBeUndefined();
          expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
          await driver.estimatesActiveRowDeleteClick();
          await expect
            .poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 })
            .toContain("Delete");
          await driver.estimatesEditDeleteConfirmDialogConfirm();
          await expect.poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 }).toBe("");
          expect(await serverEstimatesList(seed.workspaceSlug, projectId, session)).toEqual([]);
        });
      }
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-029"], "saved-system editing and single-value deletion have no OSS screen"),
  { tag: specTags(["EST-029"]) },
  async ({ driver, seed }) => {
    const tag = `NF326 noedit ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    const system = await serverCreateEstimate(seed.workspaceSlug, projectId, `${tag} prime`, ["1", "2"], session);
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
      // The only row action OSS offers is the whole-system delete: no
      // edit control exists, so no value-editing surface is reachable
      // either. The cloud confirmation contracts are the edit sibling's
      // (EST-014/EST-015), proven server-side against this same stack.
      expect(await driver.estimatesActiveRowButtonCount()).toBe(1);
      expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(false);
      await driver.estimatesActiveRowDeleteClick();
      await expect.poll(() => driver.estimatesEditDeleteConfirmDialogTitle(), { timeout: 20_000 }).toContain("Delete");
      expect(await driver.estimatesEditDeleteConfirmDialogTitle()).not.toMatch(/update/i);
      await driver.estimatesConfirmDialogCancel();
      expect(await driver.estimatesConfirmDialogOpen()).toBe(false);
      expect(await driver.estimatesActiveSystemName()).toBe(`${tag} prime`);
      expect((await serverEstimatesList(seed.workspaceSlug, projectId, session)).map((row) => row.id)).toContain(
        system.id
      );
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["EST-029"], "the estimates screen serves fully with no desktop runtime"),
  { tag: specTags(["EST-029"]) },
  async ({ driver, seed }) => {
    // Desktop pin: the parity stack has no desktop target, and the
    // desktop shell plus the instance admin carry no estimate code
    // (verified by source sweep, recorded on the row), so the scenario
    // pins the shared entry point instead — the settings route renders
    // the whole surface in a plain browser session with no desktop
    // bridge involved.
    const tag = `NF326 desktop ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N326"),
      {},
      session
    );
    const system = await serverCreateEstimate(
      seed.workspaceSlug,
      projectId,
      `${tag} prime`,
      ["Easy", "Hard"],
      session,
      "categories"
    );
    await serverSetProjectEstimate(seed.workspaceSlug, projectId, system.id, session);
    try {
      await driver.rulesEnsureSignedIn(seed.email, seed.password, seed.workspaceSlug);
      await driver.estimatesOpenSettings(seed.workspaceSlug, projectId);
      expect(await driver.estimatesNotAuthorized()).toBe(false);
      expect(await driver.estimatesHeadingVisible()).toBe(true);
      expect(await driver.estimatesEnableSwitchVisible()).toBe(true);
      await expect.poll(() => driver.estimatesActiveSystemName(), { timeout: 30_000 }).toBe(`${tag} prime`);
    } finally {
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
