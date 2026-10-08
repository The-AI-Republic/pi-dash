// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: issue detail core (NEWFRONT-121, Part A). Opening the
// full page, title editing with the save pipeline, the description editor,
// header actions, and the agent panel's empty state — against the seeded
// project, green on apps/web first. Each scenario owns its issue (created
// by name, deleted at the end) so runs stay isolated on the shared stack.
// Rows: ISS-142, ISS-143, ISS-144, ISS-145, ISS-146, ISS-147,
// ISS-160, ISS-161, ISS-162, ISS-163, ISS-164.
import { test, expect } from "../fixtures";
import {
  archivedIssueStatus,
  createProject,
  createState,
  deleteIssueStatus,
  deleteProject,
  deleteState,
  descriptionVersionDetail,
  descriptionVersions,
  fetchIssue,
  issueStatus,
  patchIssue,
  patchIssueStatus,
  patchProject,
  restoreArchivedIssue,
  signInSession,
  subscriptionStatus,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import {
  dropIssue,
  ownIssue,
  setupGuest,
  signIn,
  teardownGuest,
  type GuestUser,
  type SeedIssue,
} from "./detail-support";

test(specTitle(["ISS-142"], "open a work item full page"), { tag: specTags(["ISS-142"]) }, async ({ driver, seed }) => {
  await signIn(driver, seed);
  const session = await signInSession(seed.email, seed.password);
  const issue = await ownIssue(seed, session, `Oracle detail ${Date.now()}`);
  try {
    await test.step("open the issue detail", async () => {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    });
    await test.step("title, identifier, URL, and sidebar hydrate", async () => {
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      expect(await driver.issueDetailIdentifier()).toBe(issue.seq);
      expect(driver.page.url()).toContain(`/browse/${issue.seq}`);
      expect(await driver.sidebarProperty("State")).toContain("Todo");
    });
    await test.step("the server agrees with the screen", async () => {
      const record = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
      expect(record["name"]).toBe(issue.name);
    });
  } finally {
    await dropIssue(seed, session, issue.id);
  }
});

test(
  specTitle(["ISS-143"], "legacy short-link redirects to the detail page"),
  { tag: specTags(["ISS-143"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const target = await ownIssue(seed, session, `Oracle legacy ${Date.now()}`);
    try {
      await test.step("open the legacy route", async () => {
        await driver.openLegacyIssueRoute(seed.workspaceSlug, seed.projectId, target.id);
      });
      await test.step("lands on the browse route with the issue hydrated", async () => {
        expect(driver.page.url()).toContain(`/browse/${target.seq}`);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(target.name);
        await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
        expect(await driver.issueDetailIdentifier()).toBe(target.seq);
      });
    } finally {
      await dropIssue(seed, session, target.id);
    }
  }
);

test(
  specTitle(["ISS-144"], "detail layout and the does-not-exist empty state"),
  { tag: specTags(["ISS-144"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle layout ${Date.now()}`);
    try {
      await test.step("main content and properties sidebar render together", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
        await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
        for (const label of ["State", "Assignees", "Priority", "Created by", "Labels"]) {
          expect(await driver.sidebarProperty(label), label).not.toBeNull();
        }
      });
      await test.step("an unknown identifier shows the missing empty state", async () => {
        await driver.page.goto(`/${seed.workspaceSlug}/browse/PAR-99999`);
        await expect.poll(() => driver.seesDetailMissing(), { timeout: 30_000 }).toBe(true);
        expect(await driver.issueDetailTitle()).toBeNull();
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-145", "ISS-146"], "edit the title inline through the save pipeline"),
  { tag: specTags(["ISS-145", "ISS-146"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle rename ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      const renamed = `${issue.name} renamed`;
      await test.step("rename and watch the save indicator run", async () => {
        await driver.editIssueTitle(renamed);
        await expect.poll(() => driver.saveIndicator(), { timeout: 20_000 }).not.toBeNull();
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(renamed);
        await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      });
      await test.step("the server stored the rename", async () => {
        await expect
          .poll(async () => (await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["name"], {
            timeout: 30_000,
          })
          .toBe(renamed);
      });
      await test.step("a whitespace title is rejected and reverts", async () => {
        await driver.editIssueTitle("   ");
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(renamed);
        await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
        expect((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))["name"]).toBe(renamed);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-147"], "edit the rich-text description with autosave"),
  { tag: specTags(["ISS-147"]) },
  async ({ driver, seed }) => {
    // The version snapshot runs through the celery worker, so this scenario
    // gets a roomier budget than the suite default.
    test.setTimeout(480_000);
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle description ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("type a description and see it rendered back", async () => {
        const body = `Oracle description probe ${Date.now()}`;
        await driver.setDescription(body);
        await expect.poll(() => driver.descriptionText(), { timeout: 30_000 }).toContain(body);
        await expect
          .poll(
            async () =>
              ((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))[
                "description_html"
              ] as string) ?? "",
            { timeout: 30_000 }
          )
          .toContain(body);
      });
      await test.step("version history lists the snapshot and restores it", async () => {
        // Versions snapshot through the celery worker: back-to-back PATCHes
        // queue tasks that observe a newer DB state, so the queued snapshots
        // converge on the last text and the same-user rule merges them into
        // one row. No UI edit happens after the last PATCH, so once the row
        // converges on gamma its content is stable: worker tasks only ever
        // write version rows, never the description itself. Restoring the
        // newest version then deterministically brings gamma back over the
        // body text saved by the autosave half above.
        const beta = (i: number): string => `Oracle description beta${i} ${Date.now()}`;
        let gamma = "";
        for (let i = 1; i <= 6; i++) {
          gamma = beta(i);
          await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, {
            description_html: `<p>${gamma}</p>`,
          });
        }
        await expect
          .poll(
            async () => {
              // The list endpoint omits version content, so read the
              // newest row in full. Converged content is stable from here:
              // worker tasks only write version rows, never the description.
              const rows = await descriptionVersions(seed.workspaceSlug, seed.projectId, issue.id, session);
              if (rows.length === 0) return "";
              const newest = rows[0] as Record<string, unknown>;
              const detail = await descriptionVersionDetail(
                seed.workspaceSlug,
                seed.projectId,
                issue.id,
                String(newest["id"]),
                session
              );
              return String(detail["description_html"] ?? "");
            },
            { timeout: 180_000 }
          )
          .toContain(gamma);
        await driver.openDescriptionHistory();
        const names = await driver.historyVersionNames();
        expect(names.length).toBeGreaterThan(0);
        await driver.restoreHistoryVersion(names[0] as string);
        await expect
          .poll(
            async () =>
              ((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))[
                "description_html"
              ] as string) ?? "",
            { timeout: 30_000 }
          )
          .toContain(gamma);
        await expect.poll(() => driver.descriptionText(), { timeout: 30_000 }).toContain(gamma);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-160", "ISS-163"], "copy the work item link and identifier"),
  { tag: specTags(["ISS-160", "ISS-163"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle link ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("header copy-link copies the absolute URL with a toast", async () => {
        await driver.page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
        await driver.copyIssueLink();
        await expect.poll(() => driver.lastToast(), { timeout: 15_000 }).toMatch(/link copied/i);
        expect(await driver.readClipboard()).toContain(`/browse/${issue.seq}`);
      });
      await test.step("identifier line shows IDENT-seq", async () => {
        expect(await driver.issueDetailIdentifier()).toBe(issue.seq);
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-161"], "subscribe and unsubscribe to a work item"),
  { tag: specTags(["ISS-161"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle subscribe ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      const initial = await driver.subscribeToggle();
      expect(initial).toMatch(/^(Subscribe|Unsubscribe)$/);
      await test.step("flip the toggle and the server follows", async () => {
        await driver.clickSubscribeToggle();
        const flipped = initial === "Subscribe" ? "Unsubscribe" : "Subscribe";
        await expect.poll(() => driver.subscribeToggle(), { timeout: 15_000 }).toBe(flipped);
        await expect
          .poll(() => subscriptionStatus(seed.workspaceSlug, seed.projectId, issue.id, session), { timeout: 30_000 })
          .toBe(flipped === "Unsubscribe");
      });
      await test.step("flip it back to leave the issue as found", async () => {
        await driver.clickSubscribeToggle();
        await expect.poll(() => driver.subscribeToggle(), { timeout: 15_000 }).toBe(initial);
        await expect
          .poll(() => subscriptionStatus(seed.workspaceSlug, seed.projectId, issue.id, session), { timeout: 30_000 })
          .toBe(initial === "Unsubscribe");
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(specTitle(["ISS-162"], "detail quick-actions menu"), { tag: specTags(["ISS-162"]) }, async ({ driver, seed }) => {
  await signIn(driver, seed);
  const session = await signInSession(seed.email, seed.password);
  const issue = await ownIssue(seed, session, `Oracle actions ${Date.now()}`);
  try {
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
    await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
    const names = await driver.quickActionNames();
    expect(names).toContain("Delete");
    expect(names.some((name) => name.includes("Archive"))).toBe(true);
    expect(names).toContain("Make a copy");
    expect(names).toContain("Open in new tab");
    expect(names).toContain("Move to project");
    await driver.page.keyboard.press("Escape");
  } finally {
    await dropIssue(seed, session, issue.id);
  }
});

test(
  specTitle(["ISS-162"], "detail quick-actions delete flow"),
  { tag: specTags(["ISS-162"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle del flow ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("delete asks for confirmation naming the work item", async () => {
        await driver.clickQuickAction("Delete");
        // The modal mounts asynchronously after the menu closes.
        await expect.poll(() => driver.confirmModalText(), { timeout: 30_000 }).toContain("Delete Work item");
        await expect.poll(() => driver.confirmModalText(), { timeout: 30_000 }).toContain(issue.seq);
      });
      await test.step("confirming toasts and redirects to the issues list", async () => {
        await driver.confirmModal("Delete");
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("deleted successfully");
        await expect.poll(() => driver.page.url(), { timeout: 60_000 }).toContain(`/projects/${seed.projectId}/issues`);
      });
      await test.step("the server no longer has the issue", async () => {
        await expect
          .poll(() => issueStatus(seed.workspaceSlug, seed.projectId, issue.id, session), { timeout: 30_000 })
          .toMatchObject({ status: 404 });
      });
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["ISS-162"], "detail quick-actions archive and restore flows"),
  { tag: specTags(["ISS-162"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    // The seed project ships only an unstarted state, and archiving needs
    // a completed/cancelled one, so the scenario owns its Done state.
    const done = await createState(
      seed.workspaceSlug,
      seed.projectId,
      session,
      `Oracle Done ${Date.now()}`,
      "completed"
    );
    const issue = await ownIssue(seed, session, `Oracle arc flow ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      await test.step("archive stays disabled until the work item is done", async () => {
        expect(await driver.quickActionDisabled("Archive")).toBe(true);
        await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, { state_id: done.id });
        await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
        expect(await driver.quickActionDisabled("Archive")).toBe(false);
      });
      await test.step("archiving toasts and redirects to the issues list", async () => {
        await driver.clickQuickAction("Archive");
        // The modal mounts asynchronously after the menu closes, and it
        // renders the identifier and sequence as separate runs ("PAR 565").
        const [ident, num] = issue.seq.split("-");
        await expect.poll(() => driver.confirmModalText(), { timeout: 30_000 }).toContain("Archive Work item");
        await expect.poll(() => driver.confirmModalText(), { timeout: 30_000 }).toContain(`${ident} ${num}`);
        await driver.confirmModal("Archive");
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Archive success");
        await expect.poll(() => driver.page.url(), { timeout: 60_000 }).toContain(`/projects/${seed.projectId}/issues`);
        const archived = await archivedIssueStatus(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(archived.status).toBe(200);
        expect(archived.record?.["archived_at"]).toBeTruthy();
      });
      await test.step("the archived detail offers restore instead of archive", async () => {
        await driver.openReadOnlyIssueDetail(seed.workspaceSlug, issue.seq);
        const names = await driver.quickActionNames();
        expect(names).toContain("Restore");
        expect(names.some((name) => name.includes("Archive"))).toBe(false);
        await driver.page.keyboard.press("Escape");
      });
      await test.step("restoring toasts and navigates back to the item", async () => {
        await driver.clickQuickAction("Restore");
        await expect.poll(() => driver.lastToast(), { timeout: 30_000 }).toContain("Restore success");
        await expect.poll(() => driver.page.url(), { timeout: 60_000 }).toContain(`/browse/${issue.seq}`);
        const back = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(back["archived_at"]).toBeNull();
      });
    } finally {
      await restoreArchivedIssue(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => {});
      await dropIssue(seed, session, issue.id);
      await deleteState(seed.workspaceSlug, seed.projectId, done.id, session).catch(() => {});
    }
  }
);

test(
  specTitle(["ISS-162"], "detail quick-actions gated by role"),
  { tag: specTags(["ISS-162"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const ownerSession = await signInSession(seed.email, seed.password);
    // Guests read issues only on projects that opt into guest-wide
    // visibility, so the scenario owns a scratch project (the seed
    // project is never touched).
    // Setup lives inside the try so a mid-setup failure still tears
    // down whatever it created (guest users are invisible to issue
    // sweepers, so they must not leak).
    let project: { id: string } | null = null;
    let guest: GuestUser | null = null;
    let issue: SeedIssue | null = null;
    try {
      const stamp = Date.now();
      project = await createProject(
        seed.workspaceSlug,
        ownerSession,
        `Oracle guestproj ${stamp}`,
        `OG${String(stamp).slice(-6)}`
      );
      await patchProject(seed.workspaceSlug, project.id, ownerSession, { guest_view_all_features: true });
      guest = await setupGuest(seed, ownerSession, project.id);
      issue = await ownIssue(seed, ownerSession, `Oracle guestmenu ${stamp}`, {}, project.id);
      const projectId = project.id;
      const guestEmail = guest.email;
      const guestPassword = guest.password;
      const guestSession = guest.session;
      const issueId = issue.id;
      const issueSeq = issue.seq;
      const issueName = issue.name;
      await test.step("a guest sees only the navigation item", async () => {
        await driver.page.context().clearCookies();
        await signIn(driver, { ...seed, email: guestEmail, password: guestPassword });
        await driver.openReadOnlyIssueDetail(seed.workspaceSlug, issueSeq);
        expect(await driver.quickActionNames()).toEqual(["Open in new tab"]);
      });
      await test.step("the server rejects guest writes", async () => {
        expect(
          await patchIssueStatus(seed.workspaceSlug, projectId, issueId, guestSession, { name: "guest rename" })
        ).toBe(403);
        expect(await deleteIssueStatus(seed.workspaceSlug, projectId, issueId, guestSession)).toBe(403);
        const intact = await fetchIssue(seed.workspaceSlug, projectId, issueId, ownerSession);
        expect(intact["name"]).toBe(issueName);
      });
    } finally {
      if (project && issue) await dropIssue(seed, ownerSession, issue.id, project.id);
      if (project) await deleteProject(seed.workspaceSlug, project.id, ownerSession).catch(() => {});
      if (guest) await teardownGuest(seed, ownerSession, guest);
    }
  }
);

test(
  specTitle(["ISS-164"], "agent status panel is absent without runs"),
  { tag: specTags(["ISS-164"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle agent ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
      await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
      // No agent run or ticker exists for a fresh issue, so the status
      // panel (run status, budget, Re-tick, Abort run) renders nothing.
      expect(await driver.page.getByRole("button", { name: /re-tick/i }).count()).toBe(0);
      expect(await driver.page.getByRole("button", { name: /abort run/i }).count()).toBe(0);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
