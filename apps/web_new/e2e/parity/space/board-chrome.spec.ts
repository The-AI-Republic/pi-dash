// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios for space navbar & chrome (NEWFRONT-448): one per row
// SPC-010 through SPC-019 plus SPC-065, green on the old space app first.
// The same file must go green on apps/web_new once the spaces area lands.
// Behavior notes (learned from the old app, reference only):
// - The navbar shows the published project's name plus its configured mark
//   (icon glyph or emoji); without project details it shows a generic mark
//   and a placeholder instead of breaking the bar.
// - First visit follows the OS scheme with no stored choice. The switch
//   flips light/dark afterwards, except the first click from the system
//   default pins explicit light instead of flipping (NEWFRONT-456).
// - The product badge is pinned to the bottom-right corner on every public
//   screen and opens the product site in a new tab.
// - Anonymous sign-in and signed-in sign-out both carry the exact board
//   position (view, filters, open issue) through a return address; the API
//   honors it under the production /spaces prefix, which the parity serving
//   (space at the proxy root) does not mount — scenarios pin the carried
//   address, not the post-auth landing.
// - Inside an embedding frame the account slot and the emoji add controls
//   render nothing while the board stays readable — except the kanban
//   card-footer add control, which stays live (NEWFRONT-469).
// - Boot, board resolution, and the first issues fetch each show the logo
//   spinner; content appears only once its data is present.
// - Route transitions show a top progress bar that completes on settle;
//   copy-link raises a success notice and a failed comment post an error
//   notice. Votes raise no notice.
// - An unreachable backend shows the branded instance-failure screen with a
//   reload retry; a render crash shows the error screen with refresh plus
//   support-mail and forum links.
// - The OSS build ships English only: an unsupported stored locale falls
//   back to English and heals the stored choice; keys and date shapes never
//   pass through the translation layer.
import { test, expect } from "../fixtures";
import { browserSessionCookies, signInSession, type AuthedSession } from "../helpers/api";
import { parityApiUrl } from "../helpers/parity-env";
import { specTags, specTitle } from "../helpers/tags";
import type { ParitySeedFacts } from "../drivers/parity-driver";

function apiBase(): string {
  return parityApiUrl(process.env);
}

function requireAnchor(seed: ParitySeedFacts): string {
  const anchor = seed.spaceAnchor;
  if (!anchor) throw new Error("[parity] seed facts carry no spaceAnchor; re-run the stack seed step.");
  return anchor;
}

function setCookieLines(res: Response): string[] {
  const headers = res.headers as unknown as { getSetCookie?: () => string[] };
  if (typeof headers.getSetCookie === "function") return headers.getSetCookie();
  const single = res.headers.get("set-cookie");
  return single === null ? [] : [single];
}

function cookieHeader(lines: string[]): string {
  return lines
    .map((line) => line.split(";")[0]?.trim())
    .filter((pair) => pair !== undefined && pair.length > 0)
    .join("; ");
}

interface SpacesSession {
  cookie: string;
  location: string | null;
}

/** Spaces password sign-in exactly like the auth card: CSRF first, then the native form post. */
async function spacesSignIn(email: string, password: string, nextPath: string): Promise<SpacesSession> {
  const base = apiBase();
  const tokenRes = await fetch(`${base}/auth/get-csrf-token/`);
  if (!tokenRes.ok) throw new Error(`[parity] spaces CSRF fetch failed with HTTP ${tokenRes.status}.`);
  const payload = (await tokenRes.json()) as { csrf_token?: unknown };
  const token = typeof payload.csrf_token === "string" ? payload.csrf_token : "";
  if (token === "") throw new Error("[parity] spaces CSRF response carried no token.");
  const preCookies = cookieHeader(setCookieLines(tokenRes));
  const body = new URLSearchParams({ email, password, next_path: nextPath, csrfmiddlewaretoken: token });
  const res = await fetch(`${base}/auth/spaces/sign-in/`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded", cookie: preCookies },
    body,
    redirect: "manual",
  });
  if (res.status !== 200 && res.status !== 302) {
    throw new Error(`[parity] spaces sign-in failed with HTTP ${res.status} for ${email}.`);
  }
  const cookie = cookieHeader([...setCookieLines(tokenRes), ...setCookieLines(res)]);
  if (!cookie.includes("session-id=")) throw new Error("[parity] spaces sign-in carried no session cookie.");
  return { cookie, location: res.headers.get("location") };
}

/** Current-user identity for a cookie jar, or null when the session is gone. */
async function spacesIdentity(cookie: string): Promise<{ displayName: string; email: string } | null> {
  const res = await fetch(`${apiBase()}/api/users/me/`, { headers: { cookie } });
  if (res.status === 401) return null;
  if (!res.ok) throw new Error(`[parity] current-user fetch failed with HTTP ${res.status}.`);
  const user = (await res.json()) as { display_name?: unknown; email?: unknown };
  return {
    displayName: typeof user.display_name === "string" ? user.display_name : "",
    email: typeof user.email === "string" ? user.email : "",
  };
}

function spacesAuthed(email: string, password: string, cookie: string): AuthedSession {
  return { email, password, userId: "", cookie, csrfToken: "", apiBase: apiBase() };
}

async function publicSettings(anchor: string): Promise<{ projectName: string; raw: unknown }> {
  const res = await fetch(`${apiBase()}/api/public/anchor/${anchor}/settings/`);
  if (!res.ok) throw new Error(`[parity] publish settings fetch failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { project_details?: { name?: unknown } };
  const name = payload.project_details?.name;
  return { projectName: typeof name === "string" ? name : "", raw: payload };
}

async function publicIssues(anchor: string): Promise<Array<{ id: string; name: string }>> {
  const res = await fetch(`${apiBase()}/api/public/anchor/${anchor}/issues/`);
  if (!res.ok) throw new Error(`[parity] public issues fetch failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { results?: Array<{ id?: unknown; name?: unknown }> };
  return (payload.results ?? []).map((row) => ({
    id: typeof row.id === "string" ? row.id : "",
    name: typeof row.name === "string" ? row.name : "",
  }));
}

async function publicIssueComments(anchor: string, issueId: string): Promise<unknown[]> {
  const res = await fetch(`${apiBase()}/api/public/anchor/${anchor}/issues/${issueId}/comments/`);
  if (!res.ok) throw new Error(`[parity] public comments fetch failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as { results?: unknown[] } | unknown[];
  return Array.isArray(payload) ? payload : (payload.results ?? []);
}

async function instanceInfoOk(): Promise<boolean> {
  const res = await fetch(`${apiBase()}/api/instances/`);
  return res.ok;
}

interface BoardFlags {
  id: string;
  is_comments_enabled: boolean;
  is_reactions_enabled: boolean;
  is_votes_enabled: boolean;
}

async function deployBoard(session: string, slug: string, projectId: string): Promise<BoardFlags> {
  const res = await fetch(`${apiBase()}/api/workspaces/${slug}/projects/${projectId}/project-deploy-boards/`, {
    headers: { cookie: session },
  });
  if (!res.ok) throw new Error(`[parity] deploy-board read failed with HTTP ${res.status}.`);
  const payload = (await res.json()) as Record<string, unknown>;
  return {
    id: typeof payload["id"] === "string" ? (payload["id"] as string) : "",
    is_comments_enabled: payload["is_comments_enabled"] === true,
    is_reactions_enabled: payload["is_reactions_enabled"] === true,
    is_votes_enabled: payload["is_votes_enabled"] === true,
  };
}

async function patchBoard(
  session: string,
  slug: string,
  projectId: string,
  boardId: string,
  patch: Record<string, boolean>
): Promise<void> {
  const res = await fetch(
    `${apiBase()}/api/workspaces/${slug}/projects/${projectId}/project-deploy-boards/${boardId}/`,
    { method: "PATCH", headers: { cookie: session, "content-type": "application/json" }, body: JSON.stringify(patch) }
  );
  if (!res.ok) throw new Error(`[parity] deploy-board patch failed with HTTP ${res.status}.`);
}

/** Seeded flag baseline (all engagement off); flips heal to this first so a leaked flip never sticks. */
const BOARD_FLAG_BASELINE = { is_comments_enabled: false, is_reactions_enabled: false, is_votes_enabled: false };

async function withBoardFlags(
  seed: ParitySeedFacts,
  patch: Partial<typeof BOARD_FLAG_BASELINE>,
  body: () => Promise<void>
): Promise<void> {
  const session = await signInSession(seed.email, seed.password);
  const board = await deployBoard(session, seed.workspaceSlug, seed.projectId);
  await patchBoard(session, seed.workspaceSlug, seed.projectId, board.id, { ...BOARD_FLAG_BASELINE });
  await patchBoard(session, seed.workspaceSlug, seed.projectId, board.id, { ...patch });
  try {
    await body();
  } finally {
    await patchBoard(session, seed.workspaceSlug, seed.projectId, board.id, { ...BOARD_FLAG_BASELINE });
  }
}

test(
  specTitle(["SPC-010"], "navbar shows the project name and mark with graceful fallbacks"),
  { tag: specTags(["SPC-010"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const server = await publicSettings(anchor);
    expect(server.projectName).toBe(seed.projectName);

    await driver.spaceOpenBoard(anchor);
    const identity = await driver.spaceChromeNavbarIdentity();
    expect(identity.name).toBe(seed.projectName);
    expect(identity.logo).toBe("empty");

    const fallback = await driver.spaceChromeNavbarWithoutProjectDetails(anchor);
    expect(fallback.name).toBe("...");
    expect(fallback.logo).toBe("fallback");

    const icon = await driver.spaceChromeNavbarWithLogo(anchor, "icon");
    expect(icon.name).toBe(seed.projectName);
    expect(icon.logo).toBe("icon");

    const emoji = await driver.spaceChromeNavbarWithLogo(anchor, "emoji");
    expect(emoji.name).toBe(seed.projectName);
    expect(emoji.logo).toBe("emoji");

    // The interceptions never touched the server: the payload is intact.
    expect((await publicSettings(anchor)).projectName).toBe(seed.projectName);
  }
);

test(
  specTitle(
    ["SPC-011"],
    "bug: NEWFRONT-456 first theme toggle from the system default pins explicit light instead of flipping"
  ),
  { tag: specTags(["SPC-011"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await driver.spaceOpenBoard(anchor);
    expect(await driver.spaceChromeTheme()).toBe("light");
    const lightBg = await driver.spaceChromeBodyBackground();

    if (driver.target === "web_new") {
      // Intended (bug NEWFRONT-456 fixed): every toggle flips, including
      // the first click from the system default.
      await driver.spaceChromeToggleTheme();
      expect(await driver.spaceChromeTheme()).toBe("dark");
      const darkBg = await driver.spaceChromeBodyBackground();
      expect(darkBg).not.toBe(lightBg);

      await driver.spaceChromeToggleTheme();
      expect(await driver.spaceChromeTheme()).toBe("light");
      expect(await driver.spaceChromeBodyBackground()).toBe(lightBg);

      await driver.spaceChromeToggleTheme();
      expect(await driver.spaceChromeTheme()).toBe("dark");
      expect(await driver.spaceChromeBodyBackground()).toBe(darkBg);
      return;
    }

    await driver.spaceChromeToggleTheme();
    expect(await driver.spaceChromeTheme()).toBe("light");
    expect(await driver.spaceChromeBodyBackground()).toBe(lightBg);

    await driver.spaceChromeToggleTheme();
    expect(await driver.spaceChromeTheme()).toBe("dark");
    const darkBg = await driver.spaceChromeBodyBackground();
    expect(darkBg).not.toBe(lightBg);

    await driver.spaceChromeToggleTheme();
    expect(await driver.spaceChromeTheme()).toBe("light");
    expect(await driver.spaceChromeBodyBackground()).toBe(lightBg);
  }
);

test(
  specTitle(["SPC-011"], "first visit follows the OS scheme and the stored choice persists"),
  { tag: specTags(["SPC-011"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await driver.spaceChromeEmulateColorScheme("dark");
    await driver.spaceOpenBoard(anchor);
    expect(await driver.spaceChromeTheme()).toBe("dark");
    const darkBg = await driver.spaceChromeBodyBackground();

    await driver.spaceChromeToggleTheme();
    expect(await driver.spaceChromeTheme()).toBe("light");
    expect(await driver.spaceChromeBodyBackground()).not.toBe(darkBg);

    await driver.spaceOpenBoard(anchor);
    expect(await driver.spaceChromeTheme()).toBe("light");

    await driver.spaceChromeClearStoredTheme();
    await driver.spaceOpenBoard(anchor);
    expect(await driver.spaceChromeTheme()).toBe("dark");
  }
);

test(
  specTitle(["SPC-012"], "product badge pins to the corner and opens the site in a new tab"),
  { tag: specTags(["SPC-012"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await driver.spaceOpenBoard(anchor);
    const facts = await driver.spaceChromeBadgeFacts();
    expect(facts).not.toBeNull();
    if (facts === null) return;
    expect(facts.target).toBe("_blank");
    expect(facts.href).toBe("https://airepublic.com");
    expect(facts.box.x + facts.box.width).toBeGreaterThan(facts.viewport.width - 30);
    expect(facts.box.y + facts.box.height).toBeGreaterThan(facts.viewport.height - 40);

    const tabUrl = await driver.spaceChromeBadgeNewTabUrl();
    expect(tabUrl.replace(/\/$/, "")).toBe(facts.href.replace(/\/$/, ""));

    await driver.spaceOpenLanding();
    expect(await driver.spaceChromeBadgeFacts()).not.toBeNull();
  }
);

test(
  specTitle(["SPC-013"], "anonymous sign-in remembers the exact board position"),
  { tag: specTags(["SPC-013"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issues = await publicIssues(anchor);
    expect(issues.length).toBeGreaterThan(0);
    const issueId = issues[0]?.id ?? "";
    expect(issueId).not.toBe("");

    await driver.spaceChromeOpenIssuePeek(anchor, issueId, "?board=kanban");
    const href = await driver.spaceChromeAnonymousSignInHref();
    expect(href).not.toBeNull();
    expect(href ?? "").toContain(`/issues/${anchor}`);
    expect(href ?? "").toContain("board=kanban");
    expect(href ?? "").toContain(`peekId=${issueId}`);

    await driver.spaceOpenBoard(anchor, "?board=list&state=abc&priority=high");
    const filtered = await driver.spaceChromeAnonymousSignInHref();
    expect(filtered ?? "").toContain("state=abc");
    expect(filtered ?? "").toContain("priority=high");

    // The server honors the carried position on the way back from sign-in.
    const nextPath = `/issues/${anchor}?board=kanban&peekId=${issueId}`;
    const signed = await spacesSignIn(seed.email, seed.password, nextPath);
    expect(signed.location ?? "").toContain(`/issues/${anchor}`);
    expect(signed.location ?? "").toContain("board=kanban");
    expect(signed.location ?? "").toContain(`peekId=${issueId}`);
  }
);

test(
  specTitle(["SPC-014"], "account menu shows identity and sign-out preserves position"),
  { tag: specTags(["SPC-014"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const session = await spacesSignIn(seed.email, seed.password, "/");
    const who = await spacesIdentity(session.cookie);
    expect(who?.email).toBe(seed.email);

    await driver.openAuthenticated(
      `/issues/${anchor}?board=kanban`,
      browserSessionCookies(spacesAuthed(seed.email, seed.password, session.cookie))
    );
    const identity = await driver.spaceChromeAccountMenuIdentity();
    expect(identity.displayName).toBe(who?.displayName);
    expect(identity.offersSignOut).toBe(true);

    const form = await driver.spaceChromeSignOutForm();
    expect(form.action).toContain("/auth/spaces/sign-out/");
    expect(form.nextPath ?? "").toContain(`/issues/${anchor}`);
    expect(form.nextPath ?? "").toContain("board=kanban");

    const result = await driver.spaceChromeSignOut();
    expect(result.locationHeader ?? "").toContain(`/issues/${anchor}`);
    expect(await spacesIdentity(session.cookie)).toBeNull();

    await driver.spaceOpenBoard(anchor);
    expect(await driver.spaceChromeAnonymousSignInHref()).not.toBeNull();
  }
);

test(
  specTitle(["SPC-015"], "embedded frame hides account and emoji controls while the board stays readable"),
  { tag: specTags(["SPC-015"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await withBoardFlags(seed, { is_reactions_enabled: true }, async () => {
      const issues = await publicIssues(anchor);
      const issueId = issues[0]?.id ?? "";
      expect(issueId).not.toBe("");
      const session = await spacesSignIn(seed.email, seed.password, "/");

      await driver.openAuthenticated(
        `/issues/${anchor}?board=list`,
        browserSessionCookies(spacesAuthed(seed.email, seed.password, session.cookie))
      );
      const menu = await driver.spaceChromeAccountMenuIdentity();
      expect(menu.displayName).not.toBeNull();

      await driver.spaceChromeOpenIssuePeek(anchor, issueId);
      expect(await driver.spaceChromeEmojiControlCount()).toBeGreaterThan(0);

      await driver.spaceChromeOpenBoardFramed(anchor, `?board=list&peekId=${issueId}`);
      const framed = await driver.spaceChromeFramedFacts();
      expect(framed.sessionActiveInFrame).toBe(true);
      expect(framed.boardReadable).toBe(true);
      expect(framed.signInVisible).toBe(false);
      expect(framed.avatarVisible).toBe(false);
      expect(framed.emojiControls).toBe(0);

      expect((await publicIssues(anchor)).length).toBe(issues.length);
    });
  }
);

test(
  specTitle(["SPC-015"], "bug: NEWFRONT-469 embedded kanban footers keep a working emoji add control"),
  { tag: specTags(["SPC-015"]) },
  async ({ driver, seed }) => {
    // The overlay hides its reaction control inside a frame, but the
    // kanban card footer renders the same control with no frame gate, so
    // an embedded board keeps a live emoji add path (NEWFRONT-469).
    // Intended: no emoji add control anywhere inside a frame.
    const anchor = requireAnchor(seed);
    await withBoardFlags(seed, { is_reactions_enabled: true }, async () => {
      const issues = await publicIssues(anchor);
      expect(issues.length).toBeGreaterThan(0);
      const name = seed.issueNames[0] ?? "";
      expect(name).not.toBe("");
      const session = await spacesSignIn(seed.email, seed.password, "/");

      await driver.openAuthenticated(
        `/issues/${anchor}?board=kanban`,
        browserSessionCookies(spacesAuthed(seed.email, seed.password, session.cookie))
      );
      const footer = await driver.spaceBoardKanbanCardFooter(name);
      expect(footer.visible).toBe(true);
      expect(footer.reactions).toBe(true);
      expect(await driver.spaceChromeEmojiControlCount()).toBeGreaterThan(0);

      await driver.spaceChromeOpenBoardFramed(anchor, "?board=kanban");
      const framed = await driver.spaceChromeFramedFacts();
      expect(framed.sessionActiveInFrame).toBe(true);
      expect(framed.boardReadable).toBe(true);
      expect(framed.signInVisible).toBe(false);
      expect(framed.avatarVisible).toBe(false);
      // Intended on web_new (bug NEWFRONT-469 fixed): the frame gate hides
      // the card-footer add control too, so no emoji path stays live.
      if (driver.target === "web_new") {
        expect(framed.emojiControls).toBe(0);
      } else {
        expect(framed.emojiControls).toBeGreaterThan(0);
      }

      expect((await publicIssues(anchor)).length).toBe(issues.length);
    });
  }
);

test(
  specTitle(["SPC-016"], "boot, resolution, and first fetch gate content behind loading states"),
  { tag: specTags(["SPC-016"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    for (const stage of ["instance", "settings", "issues"] as const) {
      const probe = await driver.spaceChromeSpinnerDuring(stage, anchor);
      // Boot and first fetch spin; resolution holds the navbar placeholder
      // with empty content instead of a spinner. No stage flashes content.
      expect(probe.sawSpinner, `stage ${stage} spinner`).toBe(stage !== "settings");
      expect(probe.placeholderWhileHeld, `stage ${stage} placeholder`).toBe(stage === "settings");
      expect(probe.contentAbsentWhileSpinning, `stage ${stage} hides content while loading`).toBe(true);
      expect(probe.settledWithContent, `stage ${stage} settles with content`).toBe(true);
    }
    const issues = await publicIssues(anchor);
    expect(issues.map((row) => row.name).sort()).toEqual([...seed.issueNames].sort());
  }
);

test(
  specTitle(["SPC-017"], "slow route transitions show progress that completes on settle"),
  { tag: specTags(["SPC-017"]) },
  async ({ driver, seed }) => {
    const probe = await driver.spaceChromeProgressDuringSlowTransition(seed.workspaceSlug, seed.projectId);
    expect(probe.sawBar).toBe(true);
    expect(probe.completed).toBe(true);
  }
);

test(
  specTitle(["SPC-017"], "copying the issue link raises a success notice"),
  { tag: specTags(["SPC-017"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issues = await publicIssues(anchor);
    const issueId = issues[0]?.id ?? "";
    expect(issueId).not.toBe("");

    const toast = await driver.spaceChromeCopyIssueLinkToast(anchor, issueId);
    // Notice wording is not a parity property: each app confirms the
    // outcome in its own words. The scenario pins the success notice
    // naming the copy, plus the clipboard payload.
    expect(`${toast.title ?? ""} ${toast.message ?? ""}`).toMatch(/copied/i);
    expect(toast.clipboard ?? "").toContain(`/issues/${anchor}`);
    expect(toast.clipboard ?? "").toContain(`peekId=${issueId}`);
  }
);

test(
  specTitle(["SPC-017"], "a failed comment post raises an error notice and keeps the thread"),
  { tag: specTags(["SPC-017"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    await withBoardFlags(seed, { is_comments_enabled: true }, async () => {
      const issues = await publicIssues(anchor);
      const issueId = issues[0]?.id ?? "";
      expect(issueId).not.toBe("");
      const session = await spacesSignIn(seed.email, seed.password, "/");
      await driver.openAuthenticated(
        `/issues/${anchor}?board=list&peekId=${issueId}`,
        browserSessionCookies(spacesAuthed(seed.email, seed.password, session.cookie))
      );

      const toast = await driver.spaceChromeCommentFailureToast(anchor, issueId, "chrome notice probe");
      // Wording is each app's own (see above): the scenario pins a titled
      // error notice naming the comment, with the thread untouched.
      expect(toast.title?.trim() ?? "").not.toBe("");
      expect(`${toast.title ?? ""} ${toast.message ?? ""}`).toMatch(/comment/i);
      expect(await publicIssueComments(anchor, issueId)).toEqual([]);
    });
  }
);

test(
  specTitle(["SPC-018"], "unreachable backend shows the instance-failure screen with a reload retry"),
  { tag: specTags(["SPC-018"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const facts = await driver.spaceChromeInstanceFailureFacts(anchor);
    expect(facts.heading).toContain("Unable to fetch instance details");
    expect(facts.retryVisible).toBe(true);
    expect(facts.illustrationVisible).toBe(true);
    expect(facts.logoBarVisible).toBe(true);

    await driver.spaceChromeRetryInstanceLoad();
    expect(await instanceInfoOk()).toBe(true);
  }
);

test(
  specTitle(["SPC-019"], "a render crash shows the error screen with refresh and contact routes"),
  { tag: specTags(["SPC-019"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const facts = await driver.spaceChromeCrashFacts(anchor);
    expect(facts.heading).not.toBeNull();
    expect(facts.refreshVisible).toBe(true);
    expect(facts.supportHref ?? "").toContain("mailto:");
    expect(facts.forumHref ?? "").toContain("github.com");

    await driver.spaceChromeRefreshAfterCrash();
    expect((await publicSettings(anchor)).projectName).toBe(seed.projectName);
  }
);

test(
  specTitle(["SPC-065"], "unsupported locale falls back to English with stable keys and dates"),
  { tag: specTags(["SPC-065"]) },
  async ({ driver, seed }) => {
    const anchor = requireAnchor(seed);
    const issues = await publicIssues(anchor);
    const issueId = issues[0]?.id ?? "";
    expect(issueId).not.toBe("");

    const before = await driver.spaceLocaleBoardFacts(anchor, issueId);
    const landingBefore = await driver.spaceLocaleLandingFacts();
    expect(before.htmlLang).toBe("en");

    await driver.spaceLocaleSetStoredLocale("fr");
    const after = await driver.spaceLocaleBoardFacts(anchor, issueId);
    expect(after.peekProperties).toBe(before.peekProperties);
    expect(after.issueKeys).toEqual(before.issueKeys);
    expect(after.chromeText).toBe(before.chromeText);
    expect(after.htmlLang).toBe("en");
    expect(after.storedLocale).toBe("en");

    const landingAfter = await driver.spaceLocaleLandingFacts();
    expect(landingAfter).toEqual(landingBefore);

    await driver.spaceLocaleSetStoredLocale(null);
    const serverNames = (await publicIssues(anchor)).map((row) => row.name).sort();
    expect(serverNames).toEqual([...seed.issueNames].sort());
  }
);
