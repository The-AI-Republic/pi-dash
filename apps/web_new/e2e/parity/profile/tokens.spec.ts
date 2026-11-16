// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-429): personal access tokens from the profile
// side. The tokens tab lives under profile settings (Developer group) and
// shows the signed-in user's own tokens: a loader while the list resolves,
// an empty state carrying its own creation action, and rows with an
// active/expired marker plus a human expiry line. Creation takes a title,
// an optional description, and an expiry (preset, custom date floored at
// tomorrow, or never-expires, with a live preview), validates the title
// inline, then reveals the raw secret exactly once with copy support while
// a CSV backup downloads. Deletion asks for confirmation. Row: PROF-020.
//
// The screens are the same ones NEWFRONT-413 drives, so every step reuses
// its merged wsSettingsApiToken* driver methods and the serverApiTokens /
// serverCreateApiToken helpers; nothing here forks a parallel set. Each
// scenario provisions a fresh member (tokens are per-user, so a fresh user
// always starts empty) and deletes its tokens server-side in cleanup.
import { test, expect } from "../fixtures";
import {
  serverApiTokens,
  serverCreateApiToken,
  serverDeleteApiToken,
  serverOnboardSession,
  serverProvisionWorkspaceMember,
  sessionBrowserCookies,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROW = ["PROF-020"];

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Jun 15, 2030" — the day rendering the app's expiry lines use. */
function fmtDay(date: Date): string {
  return `${MONTHS[date.getMonth()]} ${date.getDate()}, ${date.getFullYear()}`;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Provision a fresh token owner and onboard them past the setup funnel. */
async function freshOwner(seed: { email: string; password: string; workspaceSlug: string }) {
  const ownerSession = await signInSession(seed.email, seed.password);
  const member = await serverProvisionWorkspaceMember(seed.workspaceSlug, "p429", ownerSession);
  await serverOnboardSession(member.session);
  return member;
}

/** Delete every token the fresh user owns (they are all this test's). */
async function deleteAllTokens(session: string): Promise<void> {
  const rows = await serverApiTokens(session).catch(() => []);
  for (const row of rows) await serverDeleteApiToken(row.id, session).catch(() => undefined);
}

test(
  specTitle(ROW, "loader, empty state with creation action, and the sidebar entry"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      const cookies = sessionBrowserCookies(member.session);

      await test.step("a loader shows until the token list resolves", async () => {
        await driver.wsSettingsApiTokenOpenTokens(cookies);
        expect(await driver.wsSettingsApiTokenLoaderSeen()).toBe(true);
      });

      await test.step("a fresh user sees the empty state with its creation action", async () => {
        expect(await driver.wsSettingsApiTokenHeading()).toBe("Personal Access Tokens");
        expect(await driver.wsSettingsApiTokenAddVisible()).toBe(true);
        expect(await driver.wsSettingsApiTokenEmptyVisible()).toBe(true);
        expect(await driver.wsSettingsApiTokenRows()).toEqual([]);
        expect(await serverApiTokens(member.session)).toEqual([]);
      });

      await test.step("the empty-state action opens the create dialog; cancel keeps the page empty", async () => {
        await driver.wsSettingsApiTokenEmptyCreate();
        expect(await driver.wsSettingsApiTokenDialogOpen()).toBe(true);
        await driver.wsSettingsApiTokenCancelCreate();
        expect(await driver.wsSettingsApiTokenDialogOpen()).toBe(false);
        expect(await driver.wsSettingsApiTokenEmptyVisible()).toBe(true);
        expect(await serverApiTokens(member.session)).toEqual([]);
      });

      await test.step("the profile-settings sidebar entry opens the same page", async () => {
        await driver.wsSettingsApiTokenOpenViaSidebar(cookies);
        expect(await driver.wsSettingsApiTokenHeading()).toBe("Personal Access Tokens");
        expect(await driver.wsSettingsApiTokenEmptyVisible()).toBe(true);
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(
  specTitle(ROW, "seeded tokens render as rows with expiry labels"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      const tag = `p429-list-${Date.now().toString(36)}`;
      const first = `${tag}-a`;
      const second = `${tag}-b`;

      await test.step("seed two tokens server-side, one expiring and one never", async () => {
        await serverCreateApiToken(member.session, {
          label: first,
          description: "first token",
          expiredAt: "2031-03-10T09:30:00Z",
        });
        await serverCreateApiToken(member.session, { label: second, expiredAt: null });
        expect((await serverApiTokens(member.session)).map((row) => row.label).sort()).toEqual([first, second].sort());
      });

      await test.step("the page lists both rows under the Add control", async () => {
        await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
        expect(await driver.wsSettingsApiTokenAddVisible()).toBe(true);
        expect(await driver.wsSettingsApiTokenEmptyVisible()).toBe(false);
        const rows = await driver.wsSettingsApiTokenRows();
        expect(rows.map((row) => row.label).sort()).toEqual([first, second].sort());
        const byLabel = new Map(rows.map((row) => [row.label, row]));
        expect(byLabel.get(first)).toMatchObject({
          active: true,
          expiryLine: "Expires Mar 10, 2031 at 09:30",
          description: "first token",
        });
        expect(byLabel.get(second)).toMatchObject({
          active: true,
          expiryLine: "Never expires",
          description: null,
        });
        expect(await driver.wsSettingsApiTokenPageText()).toContain(first);
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(
  specTitle(ROW, "title validation rejects empty, blank, and over-long titles"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
      await driver.wsSettingsApiTokenOpenCreate();

      await test.step("an empty title is rejected inline", async () => {
        await driver.wsSettingsApiTokenSubmit();
        await expect.poll(() => driver.wsSettingsApiTokenTitleError(), { timeout: 15_000 }).toBe("Title is required");
        expect(await driver.wsSettingsApiTokenDialogOpen()).toBe(true);
      });

      await test.step("a blank title is rejected inline", async () => {
        await driver.wsSettingsApiTokenFillTitle("   ");
        await driver.wsSettingsApiTokenSubmit();
        await expect.poll(() => driver.wsSettingsApiTokenTitleError(), { timeout: 15_000 }).toBe("Title is required");
      });

      await test.step("a 256-character title trips the length cap", async () => {
        await driver.wsSettingsApiTokenFillTitle("x".repeat(256));
        await driver.wsSettingsApiTokenSubmit();
        await expect
          .poll(() => driver.wsSettingsApiTokenTitleError(), { timeout: 15_000 })
          .toBe("Title should be less than 255 characters");
      });

      await test.step("cancel leaves no token behind", async () => {
        await driver.wsSettingsApiTokenCancelCreate();
        expect(await driver.wsSettingsApiTokenEmptyVisible()).toBe(true);
        expect(await serverApiTokens(member.session)).toEqual([]);
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(
  specTitle(ROW, "expiry presets, custom floor, never-expires toggle, live preview, missing-date error"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
      await driver.wsSettingsApiTokenOpenCreate();
      const label = `p429-expiry-${Date.now().toString(36)}`;
      await driver.wsSettingsApiTokenFillTitle(label);

      await test.step("the menu offers four presets plus custom", async () => {
        expect(await driver.wsSettingsApiTokenPresetOptions()).toEqual([
          "1 week",
          "1 month",
          "3 months",
          "1 year",
          "Custom",
        ]);
      });

      await test.step("submitting without a date reports the missing expiry", async () => {
        await driver.wsSettingsApiTokenSubmit();
        await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/Please select an expiration date/);
        expect(await driver.wsSettingsApiTokenDialogOpen()).toBe(true);
      });

      await test.step("a preset previews its resulting expiry", async () => {
        await driver.wsSettingsApiTokenPickPreset("1 week");
        expect(await driver.wsSettingsApiTokenExpiryTriggerText()).toBe("1 week");
        const inAWeek = new Date();
        inAWeek.setDate(inAWeek.getDate() + 7);
        await expect
          .poll(() => driver.wsSettingsApiTokenExpiryPreview(), { timeout: 15_000 })
          .toMatch(new RegExp(`^Expires ${escapeRegExp(fmtDay(inAWeek))} at \\d{1,2}:\\d{2}$`));
      });

      await test.step("the custom picker floors at tomorrow", async () => {
        const floor = await driver.wsSettingsApiTokenCustomFloor();
        expect(floor.todayDisabled).toBe(true);
        expect(floor.tomorrowEnabled).toBe(true);
      });

      await test.step("a custom day previews its resulting expiry", async () => {
        await driver.wsSettingsApiTokenPickCustomDayNextMonth("15");
        expect(await driver.wsSettingsApiTokenExpiryTriggerText()).toBe("Custom date");
        const now = new Date();
        const target = new Date(now.getFullYear(), now.getMonth() + 1, 15);
        await expect
          .poll(() => driver.wsSettingsApiTokenExpiryPreview(), { timeout: 15_000 })
          .toMatch(new RegExp(`^Expires ${escapeRegExp(fmtDay(target))} at \\d{1,2}:\\d{2}$`));
      });

      await test.step("never-expires disables the picker and hides the preview until flipped back", async () => {
        await driver.wsSettingsApiTokenToggleNeverExpires();
        expect(await driver.wsSettingsApiTokenExpiryTriggerDisabled()).toBe(true);
        expect(await driver.wsSettingsApiTokenExpiryPreview()).toBeNull();
        await driver.wsSettingsApiTokenToggleNeverExpires();
        expect(await driver.wsSettingsApiTokenExpiryTriggerDisabled()).toBe(false);
        const now = new Date();
        const target = new Date(now.getFullYear(), now.getMonth() + 1, 15);
        await expect
          .poll(() => driver.wsSettingsApiTokenExpiryPreview(), { timeout: 15_000 })
          .toMatch(new RegExp(`^Expires ${escapeRegExp(fmtDay(target))} at \\d{1,2}:\\d{2}$`));
      });

      await test.step("cancel leaves no token behind", async () => {
        await driver.wsSettingsApiTokenCancelCreate();
        expect(await serverApiTokens(member.session)).toEqual([]);
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(
  specTitle(ROW, "create reveals the token once with copy and a downloadable backup"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
      await driver.wsSettingsApiTokenOpenCreate();
      const label = `p429-reveal-${Date.now().toString(36)}`;
      const description = "reveal description";
      await driver.wsSettingsApiTokenFillTitle(label);
      await driver.wsSettingsApiTokenFillDescription(description);
      await driver.wsSettingsApiTokenPickPreset("1 month");

      const backup = await test.step("submit and capture the backup file", async () =>
        driver.wsSettingsApiTokenSubmitAndCaptureBackup());

      await test.step("the backup names the token and carries its secret", async () => {
        expect(backup.fileName).toMatch(/^secret-key-\d+\.csv$/);
        const [header, body] = backup.content.trim().split("\n");
        expect(header).toBe("Title,Description,Expiry,Secret key");
        expect(body).toContain(label);
        expect(body).toContain(description);
      });

      const raw = await test.step("the reveal shows the raw token once", async () => {
        expect(await driver.wsSettingsApiTokenRevealHeading()).toBe("Key created");
        const value = await driver.wsSettingsApiTokenRevealValue();
        expect(value).toMatch(/^pi_dash_api_[0-9a-f]+$/);
        expect(backup.content).toContain(value ?? "");
        const inAMonth = new Date();
        inAMonth.setMonth(inAMonth.getMonth() + 1);
        expect(await driver.wsSettingsApiTokenRevealExpiryLine()).toMatch(
          new RegExp(`^Expires ${escapeRegExp(fmtDay(inAMonth))} at \\d{1,2}:\\d{2}$`)
        );
        return value ?? "";
      });

      await test.step("copying places the raw token on the clipboard", async () => {
        expect(await driver.wsSettingsApiTokenRevealCopy()).toBe(raw);
        await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/Token copied to clipboard/);
      });

      await test.step("the server stored the metadata and expiry", async () => {
        const server = await serverApiTokens(member.session);
        expect(server).toHaveLength(1);
        expect(server[0].label).toBe(label);
        expect(server[0].description).toBe(description);
        expect(server[0].active).toBe(true);
        expect(server[0].hasToken).toBe(false);
        const stored = server[0].expiredAt === null ? Number.NaN : Date.parse(server[0].expiredAt);
        expect(stored).toBeGreaterThan(Date.now() + 27 * 24 * 3600 * 1000);
        expect(stored).toBeLessThan(Date.now() + 32 * 24 * 3600 * 1000);
      });

      await test.step("closing the reveal makes the value unrecoverable", async () => {
        await driver.wsSettingsApiTokenRevealClose();
        expect(await driver.wsSettingsApiTokenRevealValue()).toBeNull();
        expect(await driver.wsSettingsApiTokenPageText()).not.toContain(raw);
        const rows = await driver.wsSettingsApiTokenRows();
        expect(rows).toHaveLength(1);
        expect(rows[0].label).toBe(label);
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(
  specTitle(ROW, "creating with never-expires stores no expiry"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
      await driver.wsSettingsApiTokenOpenCreate();
      const label = `p429-never-${Date.now().toString(36)}`;
      await driver.wsSettingsApiTokenFillTitle(label);

      await driver.wsSettingsApiTokenToggleNeverExpires();
      const backup = await test.step("submit with never-expires on", async () =>
        driver.wsSettingsApiTokenSubmitAndCaptureBackup());

      await test.step("the reveal and backup carry no expiry", async () => {
        expect(await driver.wsSettingsApiTokenRevealHeading()).toBe("Key created");
        expect(await driver.wsSettingsApiTokenRevealExpiryLine()).toBe("Never expires");
        expect(backup.content).toContain("Never expires");
      });

      await test.step("the row and server agree on no expiry", async () => {
        await driver.wsSettingsApiTokenRevealClose();
        const rows = await driver.wsSettingsApiTokenRows();
        expect(rows).toHaveLength(1);
        expect(rows[0].label).toBe(label);
        expect(rows[0].active).toBe(true);
        expect(rows[0].expiryLine).toBe("Never expires");
        const server = await serverApiTokens(member.session);
        expect(server).toHaveLength(1);
        expect(server[0].expiredAt).toBeNull();
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(
  specTitle(ROW, "rows show liveness and expiry wording variants"),
  { tag: specTags(ROW) },
  async ({ driver, seed }) => {
    const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
    try {
      const tag = `p429-wording-${Date.now().toString(36)}`;
      const future = `${tag}-future`;
      const never = `${tag}-never`;
      const expired = `${tag}-expired`;

      await test.step("seed one token per wording variant", async () => {
        await serverCreateApiToken(member.session, {
          label: future,
          description: "future token",
          expiredAt: "2030-06-15T12:00:00Z",
        });
        await serverCreateApiToken(member.session, { label: never, expiredAt: null });
        await serverCreateApiToken(member.session, { label: expired, expiredAt: "2020-01-01T00:00:00Z" });
      });

      await test.step("each row matches its variant", async () => {
        await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
        const rows = await driver.wsSettingsApiTokenRows();
        expect(rows.map((row) => row.label).sort()).toEqual([expired, future, never].sort());
        const byLabel = new Map(rows.map((row) => [row.label, row]));
        expect(byLabel.get(future)).toMatchObject({
          active: true,
          expiryLine: "Expires Jun 15, 2030 at 12:00",
          description: "future token",
        });
        expect(byLabel.get(never)).toMatchObject({ active: true, expiryLine: "Never expires", description: null });
        const expiredRow = byLabel.get(expired);
        expect(expiredRow?.active).toBe(false);
        expect(expiredRow?.expiryLine).toMatch(/^Expired .+ ago$/);
      });

      await test.step("the server liveness agrees with the markers", async () => {
        const server = await serverApiTokens(member.session);
        const byLabel = new Map(server.map((row) => [row.label, row]));
        expect(byLabel.get(future)?.active).toBe(true);
        expect(byLabel.get(never)?.active).toBe(true);
        expect(byLabel.get(expired)?.active).toBe(false);
      });
    } finally {
      await deleteAllTokens(member.session);
    }
  }
);

test(specTitle(ROW, "deletion runs behind a confirmation dialog"), { tag: specTags(ROW) }, async ({ driver, seed }) => {
  const member = await test.step("provision a fresh token owner", async () => freshOwner(seed));
  try {
    const label = `p429-delete-${Date.now().toString(36)}`;
    await serverCreateApiToken(member.session, { label });
    await driver.wsSettingsApiTokenOpenTokens(sessionBrowserCookies(member.session));
    expect(await driver.wsSettingsApiTokenRows()).toHaveLength(1);

    await test.step("the confirmation names the irreversible action; cancel keeps the row", async () => {
      const text = await driver.wsSettingsApiTokenDeleteDialogText(label);
      expect(text).toMatch(/Delete personal access token/);
      expect(text).toMatch(/cannot be undone/);
      await driver.wsSettingsApiTokenDeleteCancel();
      expect(await driver.wsSettingsApiTokenRows()).toHaveLength(1);
      expect(await serverApiTokens(member.session)).toHaveLength(1);
    });

    await test.step("confirming deletes with a notice and clears server state", async () => {
      await driver.wsSettingsApiTokenDeleteConfirm(label);
      await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/successfully deleted/);
      expect(await driver.wsSettingsApiTokenRows()).toEqual([]);
      expect(await driver.wsSettingsApiTokenEmptyVisible()).toBe(true);
      expect(await serverApiTokens(member.session)).toEqual([]);
    });
  } finally {
    await deleteAllTokens(member.session);
  }
});
