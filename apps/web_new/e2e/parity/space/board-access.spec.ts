// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-447): space board access and routing,
// rows SPC-001 (anonymous anchor load), SPC-002 (legacy link resolution),
// SPC-003 (unknown/withdrawn anchor illustration), SPC-004 (board-error
// illustration on settings/issues failure), SPC-005 (landing auth gate),
// SPC-006 (return-address validation), SPC-007 (per-board metadata,
// bug NEWFRONT-453: the loader cannot resolve its API base under the
// default config, so every board falls back to the generic tags),
// SPC-008 (robots directive), SPC-009 (unmatched-path page).
//
// Observed behavior notes (this checkout): an anchor address renders the
// board shell with the project name, a position-preserving sign-in link,
// and the published issues without any account; legacy two-segment links
// forward to the anchor address carrying the view and open-issue
// parameters, and land on the generic guidance page for unpublished
// projects. Unknown anchors show a dedicated 404 illustration while any
// other settings failure (and any issues-list failure) shows the generic
// board-error illustration. Client navigation normalizes board addresses
// with a trailing slash. The landing root shows the email-first auth
// form to anonymous visitors and a paste-a-link guidance view to
// signed-in ones, returning the latter to a valid next_path; hostile
// return addresses (absolute, protocol-relative, scheme, backslash,
// missing-slash) are dropped and the visitor stays on the landing page.
// Served pages carry noindex/nofollow; unmatched paths render the generic
// guidance page. The served head always carries the generic
// title/description because the metadata loader throws on its relative
// API base (bug NEWFRONT-453); a control run under an absolute base
// renders the board name and, when a cover is set, cover image tags that
// the SPC-007 target (main#523) removes.
import { test, expect } from "../fixtures";
import {
  browserSessionCookies,
  ownerSession,
  signInSession,
  serverCreateProject,
  serverDeleteProject,
  serverSpaceAnchorForProject,
  serverSpaceBoardIssueNames,
  serverSpaceBoardMeta,
  serverSpaceBoardSettings,
  serverSpaceDeleteBoard,
  serverSpaceMeStatus,
  serverSpacePublishBoard,
  type AuthedSession,
} from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";

/** Seeded board anchor; every scenario in this file needs it. */
function requireAnchor(seed: { spaceAnchor?: string }): string {
  if (!seed.spaceAnchor) throw new Error("[parity] seed carries no spaceAnchor; reseed the stack.");
  return seed.spaceAnchor;
}

/** Seed-admin session shaped for cookie-jar building (cf. PROF-010). */
async function seedAuthed(email: string, password: string): Promise<AuthedSession> {
  const cookie = await signInSession(email, password);
  const csrfToken = /(?:^|;\s*)csrftoken=([^;]*)/.exec(cookie)?.[1] ?? "";
  return { email, password, userId: "", cookie, csrfToken, apiBase: parityApiUrl(process.env) };
}

/** Path portion of a currentPath value (drops the query string). */
function pathOf(currentPath: string): string {
  return currentPath.split("?", 1)[0] ?? currentPath;
}

/** Scratch-project name/identifier pair, unique per call (alnum only: names reject special characters). */
function scratchProject(tag: string): { name: string; identifier: string } {
  const suffix = Date.now().toString(36).toUpperCase().slice(-6);
  const stem = `SPC${tag}${suffix}`.replace(/[^a-zA-Z0-9]/g, "");
  return { name: stem, identifier: `SA${suffix}`.slice(0, 8) };
}

test(
  specTitle(["SPC-001"], "anchor address opens the board anonymously with no sign-in"),
  { tag: specTags(["SPC-001"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("server publishes the board with the seeded issues", async () => {
      const settings = await serverSpaceBoardSettings(anchor);
      expect(settings.status).toBe(200);
      const payload = settings.payload as { entity_name?: unknown; project_details?: { name?: unknown } };
      expect(payload.entity_name).toBe("project");
      expect(payload.project_details?.name).toBe(seed.projectName);
      const issues = await serverSpaceBoardIssueNames(anchor);
      expect(issues.status).toBe(200);
      expect(issues.totalCount).toBeGreaterThan(0);
      for (const name of seed.issueNames) expect(issues.names).toContain(name);
    });

    await test.step("anonymous load renders the board without leaving the anchor address", async () => {
      await driver.spaceOpenBoard(anchor);
      await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
      // Client navigation normalizes the board address with a trailing slash.
      expect(pathOf(await driver.currentPath())).toBe(`/issues/${anchor}/`);
      for (const name of seed.issueNames) {
        await expect.poll(() => driver.spaceAccessBoardShowsIssue(name), { timeout: 60_000 }).toBe(true);
      }
      expect(await driver.spaceAccessAnchorNotFoundVisible()).toBe(false);
      expect(await driver.spaceAccessBoardErrorVisible()).toBe(false);
      expect(await driver.spaceAccessGenericNotFoundVisible()).toBe(false);
    });

    await test.step("navbar sign-in remembers the board position", async () => {
      const href = await driver.spaceAccessSignInHref();
      expect(href).not.toBeNull();
      expect(href ?? "").toContain("next_path=");
      expect(href ?? "").toContain(`/issues/${anchor}`);
    });
  }
);

test(
  specTitle(["SPC-002"], "legacy workspace/project link resolves to the anchor board carrying view and peek"),
  { tag: specTags(["SPC-002"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("server resolves the project to its anchor", async () => {
      const resolved = await serverSpaceAnchorForProject(seed.workspaceSlug, seed.projectId);
      expect(resolved.status).toBe(200);
      expect((resolved.payload as { anchor?: unknown }).anchor).toBe(anchor);
    });

    await test.step("published project forwards with view and open issue preserved", async () => {
      const issues = await serverSpaceBoardIssueNames(anchor);
      expect(issues.ids.length).toBeGreaterThan(0);
      const peekId = issues.ids[0] as string;
      await driver.spaceAccessOpenPath(`/${seed.workspaceSlug}/${seed.projectId}?board=kanban&peekId=${peekId}`);
      await expect.poll(async () => pathOf(await driver.currentPath()), { timeout: 60_000 }).toBe(`/issues/${anchor}/`);
      const landed = new URL(await driver.currentPath(), "http://localhost");
      expect(landed.searchParams.get("board")).toBe("kanban");
      expect(landed.searchParams.get("peekId")).toBe(peekId);
      await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
    });

    await test.step("unpublished project lands on the generic not-found page", async () => {
      const admin = await ownerSession(seed);
      const scratch = scratchProject("legacy-missing");
      const projectId = await serverCreateProject(seed.workspaceSlug, admin, scratch.name, scratch.identifier);
      try {
        const missing = await serverSpaceAnchorForProject(seed.workspaceSlug, projectId);
        expect(missing.status).toBe(404);
        await driver.spaceOpenLegacyLink(seed.workspaceSlug, projectId);
        await expect.poll(() => driver.currentPath(), { timeout: 60_000 }).toBe("/404");
        await expect.poll(() => driver.spaceAccessGenericNotFoundVisible(), { timeout: 60_000 }).toBe(true);
      } finally {
        await serverDeleteProject(seed.workspaceSlug, projectId, admin);
      }
    });
  }
);

test(
  specTitle(["SPC-003"], "unknown or withdrawn anchor shows the illustrated not-found page"),
  { tag: specTags(["SPC-003"]) },
  async ({ driver, seed }) => {
    await test.step("unknown anchor renders the not-found illustration, not the board chrome", async () => {
      const bogus = "0".repeat(32);
      expect((await serverSpaceBoardSettings(bogus)).status).toBe(404);
      await driver.spaceOpenBoard(bogus);
      await expect.poll(() => driver.spaceAccessAnchorNotFoundVisible(), { timeout: 60_000 }).toBe(true);
      expect(await driver.spaceAccessBoardShowsProject(seed.projectName)).toBe(false);
      expect(await driver.spaceAccessBoardErrorVisible()).toBe(false);
      expect(await driver.spaceAccessGenericNotFoundVisible()).toBe(false);
    });

    await test.step("withdrawn anchor renders the same illustration", async () => {
      const admin = await ownerSession(seed);
      const scratch = scratchProject("withdrawn");
      const projectId = await serverCreateProject(seed.workspaceSlug, admin, scratch.name, scratch.identifier);
      try {
        const board = await serverSpacePublishBoard(seed.workspaceSlug, projectId, admin);
        expect((await serverSpaceBoardSettings(board.anchor)).status).toBe(200);
        await serverSpaceDeleteBoard(seed.workspaceSlug, projectId, board.id, admin);
        expect((await serverSpaceBoardSettings(board.anchor)).status).toBe(404);
        await driver.spaceOpenBoard(board.anchor);
        await expect.poll(() => driver.spaceAccessAnchorNotFoundVisible(), { timeout: 60_000 }).toBe(true);
      } finally {
        await serverDeleteProject(seed.workspaceSlug, projectId, admin);
      }
    });
  }
);

test(
  specTitle(["SPC-004"], "board settings or issues failure shows the board-error illustration"),
  { tag: specTags(["SPC-004"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("settings failure renders the error illustration instead of the board", async () => {
      await driver.spaceAccessFailBoardSettings(500);
      try {
        await driver.spaceOpenBoard(anchor);
        await expect.poll(() => driver.spaceAccessBoardErrorVisible(), { timeout: 60_000 }).toBe(true);
        expect(await driver.spaceAccessAnchorNotFoundVisible()).toBe(false);
        expect(await driver.spaceAccessBoardShowsProject(seed.projectName)).toBe(false);
      } finally {
        await driver.spaceAccessClearFailures();
      }
    });

    await test.step("issues failure renders the error illustration under the board chrome", async () => {
      await driver.spaceAccessFailBoardIssues(500);
      try {
        await driver.spaceOpenBoard(anchor);
        await expect.poll(() => driver.spaceAccessBoardErrorVisible(), { timeout: 60_000 }).toBe(true);
        expect(await driver.spaceAccessAnchorNotFoundVisible()).toBe(false);
        await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
      } finally {
        await driver.spaceAccessClearFailures();
      }
    });

    await test.step("backend itself stays healthy", async () => {
      expect((await serverSpaceBoardSettings(anchor)).status).toBe(200);
      expect((await serverSpaceBoardIssueNames(anchor)).status).toBe(200);
    });
  }
);

test(
  specTitle(["SPC-005"], "landing page gates on auth: anonymous forms, signed-in guidance, next_path return"),
  { tag: specTags(["SPC-005"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("anonymous visitors get the auth forms", async () => {
      expect(await serverSpaceMeStatus(null)).not.toBe(200);
      await driver.spaceOpenLanding();
      await expect.poll(() => driver.spaceAccessLandingView(), { timeout: 60_000 }).toBe("auth");
    });

    await test.step("signed-in visitors get guidance to paste a public link", async () => {
      const session = await seedAuthed(seed.email, seed.password);
      expect(await serverSpaceMeStatus(session.cookie)).toBe(200);
      await driver.spaceAccessOpenLandingAuthed(browserSessionCookies(session));
      await expect.poll(() => driver.spaceAccessLandingView(), { timeout: 60_000 }).toBe("signed-in");
    });

    await test.step("a valid next_path returns a signed-in visitor to the board", async () => {
      const session = await seedAuthed(seed.email, seed.password);
      await driver.spaceAccessOpenLandingAuthed(
        browserSessionCookies(session),
        `?next_path=${encodeURIComponent(`/issues/${anchor}`)}`
      );
      await expect.poll(async () => pathOf(await driver.currentPath()), { timeout: 60_000 }).toBe(`/issues/${anchor}/`);
      await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
    });
  }
);

test(
  specTitle(["SPC-006"], "sign-in return addresses keep in-app paths only"),
  { tag: specTags(["SPC-006"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const hostile = [
      "https://evil.example/issues/x",
      "//evil.example/issues/x",
      "javascript:alert(1)",
      "\\\\evil\\path",
      "issues/x",
    ];

    await test.step("hostile return addresses are dropped and the visitor stays on the landing page", async () => {
      const session = await seedAuthed(seed.email, seed.password);
      expect(await serverSpaceMeStatus(session.cookie)).toBe(200);
      for (const bad of hostile) {
        await driver.spaceAccessOpenLandingAuthed(
          browserSessionCookies(session),
          `?next_path=${encodeURIComponent(bad)}`
        );
        await expect.poll(() => driver.spaceAccessLandingView(), { timeout: 60_000 }).toBe("signed-in");
        expect(pathOf(await driver.currentPath())).toBe("/");
      }
    });

    await test.step("a valid in-app return address with its own query still returns", async () => {
      const session = await seedAuthed(seed.email, seed.password);
      await driver.spaceAccessOpenLandingAuthed(
        browserSessionCookies(session),
        `?next_path=${encodeURIComponent(`/issues/${anchor}?board=kanban`)}`
      );
      await expect.poll(async () => pathOf(await driver.currentPath()), { timeout: 60_000 }).toBe(`/issues/${anchor}/`);
      const landed = new URL(await driver.currentPath(), "http://localhost");
      expect(landed.searchParams.get("board")).toBe("kanban");
      expect((await serverSpaceBoardSettings(anchor)).status).toBe(200);
    });
  }
);

test(
  specTitle(["SPC-007"], "bug: NEWFRONT-453 board metadata always falls back to the generic tags"),
  { tag: specTags(["SPC-007"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("server carries per-board metadata", async () => {
      const meta = await serverSpaceBoardMeta(anchor);
      expect(meta.status).toBe(200);
      expect((meta.payload as { name?: unknown }).name).toBe(seed.projectName);
    });

    if (driver.target === "web_new") {
      await test.step("board page renders the per-board metadata with no cover image", async () => {
        // Intended (bug NEWFRONT-453 fixed): the board name unfurls with no
        // cover image (target per main#523). The seeded board carries no
        // summary, so the description falls back to the generic line.
        await driver.spaceOpenBoard(anchor);
        await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
        const head = await driver.spaceAccessHeadFacts();
        expect(head.title).toBe(seed.projectName);
        expect(head.description).toBe("Projects, issues and agents in one place.");
        expect(head.ogTitle).toBe(seed.projectName);
        expect(head.ogDescription).toBe("Projects, issues and agents in one place.");
        expect(head.twitterTitle).toBe(seed.projectName);
        expect(head.ogImage).toBeNull();
        expect(head.twitterImage).toBeNull();
      });
    } else {
      await test.step("board page renders the generic fallback instead of the board metadata", async () => {
        // Current behavior only (bug NEWFRONT-453): the metadata loader
        // throws on its relative API base, so every board serves the generic
        // tags. Intended: the board name/summary unfurl with no cover image.
        await driver.spaceOpenBoard(anchor);
        await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
        const head = await driver.spaceAccessHeadFacts();
        expect(head.title).toBe("Pi Dash");
        expect(head.description).toBe(
          "Made with Pi Dash, an AI-powered work management platform with publishing capabilities."
        );
        expect(head.ogTitle).toBe("Pi Dash");
        expect(head.ogDescription).toBe(
          "Made with Pi Dash, an AI-powered work management platform with publishing capabilities."
        );
        expect(head.twitterTitle).toBe("Pi Dash");
        expect(head.ogImage).toBeNull();
        expect(head.twitterImage).toBeNull();
      });
    }

    await test.step("malformed anchors also fall back", async () => {
      expect((await serverSpaceBoardMeta("!!!")).status).toBe(404);
      await driver.spaceOpenBoard("!!!");
      await expect.poll(() => driver.spaceAccessAnchorNotFoundVisible(), { timeout: 60_000 }).toBe(true);
      const head = await driver.spaceAccessHeadFacts();
      // Intended on web_new: the dedicated not-found title (no board tags),
      // never the generic product title and never a cover image.
      expect(head.title).toBe(driver.target === "web_new" ? "Pi Dash — Board not found" : "Pi Dash");
      expect(head.ogImage).toBeNull();
      expect(head.twitterImage).toBeNull();
    });
  }
);

test(
  specTitle(["SPC-008"], "board pages carry a noindex/nofollow robots directive"),
  { tag: specTags(["SPC-008"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("board page asks crawlers not to index it", async () => {
      expect((await serverSpaceBoardSettings(anchor)).status).toBe(200);
      await driver.spaceOpenBoard(anchor);
      await expect.poll(() => driver.spaceAccessBoardShowsProject(seed.projectName), { timeout: 60_000 }).toBe(true);
      expect((await driver.spaceAccessHeadFacts()).robots).toBe("noindex, nofollow");
    });

    await test.step("landing page carries the same directive", async () => {
      await driver.spaceOpenLanding();
      await expect.poll(() => driver.spaceAccessLandingView(), { timeout: 60_000 }).toBe("auth");
      expect((await driver.spaceAccessHeadFacts()).robots).toBe("noindex, nofollow");
    });
  }
);

test(
  specTitle(["SPC-009"], "unmatched paths show the generic not-found page"),
  { tag: specTags(["SPC-009"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);

    await test.step("backend is healthy, so the 404 comes from routing", async () => {
      expect((await serverSpaceBoardSettings(anchor)).status).toBe(200);
    });

    for (const unknown of ["/no-such-space-page-447", "/deep/nested/unknown/path"]) {
      await test.step(`unknown path ${unknown} renders the generic guidance page`, async () => {
        await driver.spaceAccessOpenPath(unknown);
        await expect.poll(() => driver.spaceAccessGenericNotFoundVisible(), { timeout: 60_000 }).toBe(true);
        expect(await driver.spaceAccessAnchorNotFoundVisible()).toBe(false);
        expect(await driver.spaceAccessBoardErrorVisible()).toBe(false);
      });
    }
  }
);
