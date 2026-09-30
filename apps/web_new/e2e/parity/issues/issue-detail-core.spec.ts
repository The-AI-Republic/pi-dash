// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios: issue detail core (NEWFRONT-121, Part A). Opening the
// full page, title editing with the save pipeline, the description editor,
// header actions, and the agent panel's empty state — against the seeded
// project, green on apps/web first. Each scenario owns its issue (created
// by name, deleted at the end) so runs stay isolated on the shared stack.
// Rows: ISS-142, ISS-143, ISS-144, ISS-145, ISS-146, ISS-147,
// ISS-160, ISS-161, ISS-162, ISS-163, ISS-164.
import { test, expect } from "../fixtures";
import { descriptionVersions, fetchIssue, patchIssue, signInSession, subscriptionStatus } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";
import { dropIssue, ownIssue, signIn } from "./detail-support";

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
      const renamed = `${issue.name} renamed`;
      await test.step("rename and watch the save indicator run", async () => {
        await driver.editIssueTitle(renamed);
        await expect.poll(() => driver.saveIndicator(), { timeout: 20_000 }).not.toBeNull();
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(renamed);
      });
      await test.step("the server stored the rename", async () => {
        const record = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
        expect(record["name"]).toBe(renamed);
      });
      await test.step("a whitespace title is rejected and reverts", async () => {
        await driver.editIssueTitle("   ");
        await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(renamed);
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
    test.setTimeout(300_000);
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle description ${Date.now()}`);
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 30_000 }).toBe(issue.name);
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
        // Versions snapshot asynchronously: back-to-back PATCHes queue
        // worker tasks that observe a newer DB state, which creates the
        // version, and the same-user rule converges its content to the
        // last text. Restore then brings exactly that text back.
        const beta = (i: number): string => `Oracle description beta${i} ${Date.now()}`;
        let gamma = "";
        for (let i = 1; i <= 6; i++) {
          gamma = beta(i);
          await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, {
            description_html: `<p>${gamma}</p>`,
          });
        }
        await expect
          .poll(async () => (await descriptionVersions(seed.workspaceSlug, seed.projectId, issue.id, session)).length, {
            timeout: 90_000,
          })
          .toBeGreaterThan(0);
        const delta = `Oracle description delta ${Date.now()}`;
        await driver.setDescription(delta);
        await expect
          .poll(
            async () =>
              ((await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session))[
                "description_html"
              ] as string) ?? "",
            { timeout: 30_000 }
          )
          .toContain(delta);
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
      await test.step("header copy-link copies the absolute URL with a toast", async () => {
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
      const initial = await driver.subscribeToggle();
      expect(initial).toMatch(/^(Subscribe|Unsubscribe)$/);
      await test.step("flip the toggle and the server follows", async () => {
        await driver.clickSubscribeToggle();
        const flipped = initial === "Subscribe" ? "Unsubscribe" : "Subscribe";
        await expect.poll(() => driver.subscribeToggle(), { timeout: 15_000 }).toBe(flipped);
        expect(await subscriptionStatus(seed.workspaceSlug, seed.projectId, issue.id, session)).toBe(
          flipped === "Unsubscribe"
        );
      });
      await test.step("flip it back to leave the issue as found", async () => {
        await driver.clickSubscribeToggle();
        await expect.poll(() => driver.subscribeToggle(), { timeout: 15_000 }).toBe(initial);
        expect(await subscriptionStatus(seed.workspaceSlug, seed.projectId, issue.id, session)).toBe(
          initial === "Unsubscribe"
        );
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
    const names = await driver.quickActionNames();
    expect(names).toContain("Delete");
    expect(names.some((name) => name.includes("Archive"))).toBe(true);
    await driver.page.keyboard.press("Escape");
  } finally {
    await dropIssue(seed, session, issue.id);
  }
});

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
      // No agent run or ticker exists for a fresh issue, so the status
      // panel (run status, budget, Re-tick, Abort run) renders nothing.
      expect(await driver.page.getByRole("button", { name: /re-tick/i }).count()).toBe(0);
      expect(await driver.page.getByRole("button", { name: /abort run/i }).count()).toBe(0);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
