// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-427): the preferences tab applies theme picks
// across the app and persists them, the custom-color designer previews live
// and saves with a reload, and the language/timezone/week-start selectors
// save on pick and reshape other screens. Row PROF-018 only proves the
// notifications tab renders its email entry; the toggle depth lives in
// notifications/email-preferences.spec.ts and email-delivery.spec.ts
// (NTF-024/025, NEWFRONT-201). Rows: PROF-016, PROF-017, PROF-018.
//
// Every scenario restores the seed theme/locale it flipped, so reruns on one
// seeded stack stay green whatever earlier runs left behind.
import { test, expect } from "../fixtures";
import {
  serverCreateCycle,
  serverCycleDetail,
  serverDeleteCycle,
  serverEmailPreferences,
  serverPatchProfile,
  serverProfileLocale,
  serverProfileTheme,
  serverSessionUserId,
  signInSession,
} from "../helpers/api";
import type { SettingsProfileTheme } from "../drivers/parity-driver";
import { specTags, specTitle } from "../helpers/tags";

const ROWS_016 = ["PROF-016"];
const ROWS_017 = ["PROF-017"];
const ROWS_018 = ["PROF-018"];

/** Preset themes with the document marker each one applies. */
const hexToRgb = (hex: string): string => {
  const clean = hex.replace("#", "");
  const r = parseInt(clean.slice(0, 2), 16);
  const g = parseInt(clean.slice(2, 4), 16);
  const b = parseInt(clean.slice(4, 6), 16);
  return `rgb(${r},${g},${b})`;
};

const PRESETS: { theme: SettingsProfileTheme; marker: string | "system" }[] = [
  { theme: "system", marker: "system" },
  { theme: "light", marker: "light" },
  { theme: "dark", marker: "dark" },
  { theme: "light-contrast", marker: "light-contrast" },
  { theme: "dark-contrast", marker: "dark-contrast" },
];

test(
  specTitle(ROWS_016, "preset themes apply instantly and persist across reload"),
  { tag: specTags(ROWS_016) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const initialRaw = await serverProfileTheme(session);

    await driver.settingsProfileOpenPreferences();
    const initial = await driver.settingsProfileThemeValue();

    for (const { theme, marker } of PRESETS) {
      if (theme === initial) continue;
      await test.step(`picking ${theme} applies and persists it`, async () => {
        await driver.settingsProfileSetTheme(theme);
        expect(await driver.settingsProfileThemeValue()).toBe(theme);
        // The CE switcher sets the theme on pick, then reloads to apply the
        // saved theme; past the reload the marker and the server agree.
        const applied = await driver.settingsProfileAppliedTheme();
        if (marker === "system") {
          expect(["light", "dark"]).toContain(applied);
        } else {
          expect(applied).toBe(marker);
        }
        expect((await serverProfileTheme(session)).theme).toBe(theme);
      });
    }

    await test.step("restore the initial theme", async () => {
      if (initial === null) {
        // The switcher offers no unset, so restore through the API. No UI
        // re-read follows: the profile GET is HTTP-cached, so the running
        // page would keep showing the last pick; the next test boots fresh.
        expect((await serverPatchProfile(session, { theme: initialRaw })).ok).toBe(true);
      } else {
        await driver.settingsProfileSetTheme(initial);
        expect(await driver.settingsProfileThemeValue()).toBe(initial);
      }
      expect(await serverProfileTheme(session)).toEqual(initialRaw);
    });
  }
);

test(
  specTitle(ROWS_016, "custom designer previews live and saves with reload-to-apply"),
  { tag: specTags(ROWS_016) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const initialRaw = await serverProfileTheme(session);

    await driver.settingsProfileOpenPreferences();
    const initial = await driver.settingsProfileThemeValue();
    if (initial !== "custom") {
      await driver.settingsProfileSetTheme("custom");
    }
    expect(await driver.settingsProfileCustomDesignerVisible()).toBe(true);

    // Preview colors that differ from whatever the designer currently
    // holds, so the live swatch always has a change to show.
    const current = await driver.settingsProfileCustomDesignerValues();
    const primary = current.primary.toLowerCase() === "#ff5151" ? "#00aa55" : "#ff5151";
    const background = current.background.toLowerCase() === "#1a2b3c" ? "#3c2b1a" : "#1a2b3c";
    await test.step("typing colors previews them before any save", async () => {
      const primarySwatch = await driver.settingsProfilePreviewCustomColor("primary", primary);
      expect(primarySwatch?.replace(/\s+/g, "").toLowerCase()).toBe(hexToRgb(primary));
      const backgroundSwatch = await driver.settingsProfilePreviewCustomColor("background", background);
      expect(backgroundSwatch?.replace(/\s+/g, "").toLowerCase()).toBe(hexToRgb(background));
      // Nothing saved yet: the server still holds the pre-design values.
      expect(await serverProfileTheme(session)).toEqual({
        ...initialRaw,
        theme: "custom",
      });
    });

    await test.step("saving applies the custom theme after a reload", async () => {
      await driver.settingsProfileSetCustomPalette(true);
      await driver.settingsProfileSaveCustomTheme();
      expect(await driver.settingsProfileThemeValue()).toBe("custom");
      expect(await driver.settingsProfileAppliedTheme()).toBe("custom");
      expect(await driver.settingsProfileCustomVarsApplied()).toBe(true);
      expect(await serverProfileTheme(session)).toEqual({
        theme: "custom",
        primary,
        background,
        darkPalette: true,
      });
      // The reloaded designer shows the saved colors, not the defaults.
      expect(await driver.settingsProfileCustomDesignerValues()).toEqual({
        primary,
        background,
        darkPalette: true,
      });
    });

    await test.step("restore the initial theme", async () => {
      // API-only restore (see the preset scenario): no UI re-read, because
      // the profile GET is HTTP-cached for the running page.
      expect((await serverPatchProfile(session, { theme: initialRaw })).ok).toBe(true);
      expect(await serverProfileTheme(session)).toEqual(initialRaw);
    });
  }
);

test(
  specTitle(ROWS_016, "re-picking custom re-applies saved custom colors"),
  { tag: specTags(ROWS_016) },
  async ({ driver, seed }) => {
    // Server setup first: the profile GET is HTTP-cached, so the browser
    // must boot from the staged state rather than read it mid-test.
    const session = await signInSession(seed.email, seed.password);
    const initialRaw = await serverProfileTheme(session);
    const saved = { theme: "light", primary: "#112233", background: "#445566", darkPalette: true };
    expect((await serverPatchProfile(session, { theme: saved })).ok).toBe(true);

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    await driver.settingsProfileOpenPreferences();
    expect(await driver.settingsProfileThemeValue()).toBe("light");
    expect(await driver.settingsProfileCustomVarsApplied()).toBe(false);

    await test.step("picking custom applies the saved colors", async () => {
      await driver.settingsProfileSetTheme("custom");
      expect(await driver.settingsProfileThemeValue()).toBe("custom");
      expect(await driver.settingsProfileAppliedTheme()).toBe("custom");
      expect(await driver.settingsProfileCustomVarsApplied()).toBe(true);
      expect(await serverProfileTheme(session)).toEqual({ ...saved, theme: "custom" });
      expect(await driver.settingsProfileCustomDesignerValues()).toEqual({
        primary: saved.primary,
        background: saved.background,
        darkPalette: saved.darkPalette,
      });
    });

    await test.step("restore the initial theme", async () => {
      expect((await serverPatchProfile(session, { theme: initialRaw })).ok).toBe(true);
      expect(await serverProfileTheme(session)).toEqual(initialRaw);
    });
  }
);

// A failed save reports the error and the store rolls the switcher back,
// but the already-applied document theme stays on the failed pick instead
// of rolling back with it. This scenario pins that observed split; the row
// records the intended full rollback with the linked issue.
test(
  specTitle(
    ROWS_016,
    "bug: NEWFRONT-434 a failed theme save leaves the new theme applied while the switcher rolls back"
  ),
  { tag: specTags(ROWS_016) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const initialRaw = await serverProfileTheme(session);

    await driver.settingsProfileOpenPreferences();
    if ((await driver.settingsProfileThemeValue()) !== "light") {
      await driver.settingsProfileSetTheme("light");
    }

    await driver.settingsProfileFailProfileSaves(500);
    try {
      await test.step("the failed pick reports and rolls back without reloading", async () => {
        await driver.settingsProfileSetTheme("dark");
        if (driver.target === "web_new") {
          // Target per NEWFRONT-434 (PROF-016): the new app reports with
          // its own copy and rolls the failed pick back completely.
          await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/couldn't update the theme/i);
        } else {
          await expect.poll(() => driver.toastText(), { timeout: 15_000 }).toMatch(/failed to update theme/i);
        }
        // The store rolled back: the switcher shows light again and the
        // server never saw dark...
        expect(await driver.settingsProfileThemeValue()).toBe("light");
        expect((await serverProfileTheme(session)).theme).toBe("light");
        if (driver.target === "web_new") {
          // ...and the document rolled back with it (the full-rollback
          // target; the oracle leg below pins the observed split).
          expect(await driver.settingsProfileAppliedTheme()).toBe("light");
        } else {
          // ...while the document still shows the theme that never saved.
          expect(await driver.settingsProfileAppliedTheme()).toBe("dark");
        }
      });
    } finally {
      await driver.settingsProfileClearProfileSaveFailure();
    }

    await test.step("a retry past the failure saves normally", async () => {
      await driver.settingsProfileSetTheme("dark");
      expect((await serverProfileTheme(session)).theme).toBe("dark");
    });

    await test.step("restore the initial theme", async () => {
      expect((await serverPatchProfile(session, { theme: initialRaw })).ok).toBe(true);
      expect(await serverProfileTheme(session)).toEqual(initialRaw);
    });
  }
);

test(
  specTitle(ROWS_017, "timezone shifts clocks across screens"),
  { tag: specTags(ROWS_017) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const userId = await serverSessionUserId(session);
    const initialTz = (await serverProfileLocale(session)).user_timezone ?? "UTC";

    const clockOf = (text: string | null): string => {
      const match = text === null ? null : /(\d{2}:\d{2})/.exec(text);
      expect(match, `expected a clock reading in ${JSON.stringify(text)}`).not.toBeNull();
      return match![1]!;
    };

    await driver.settingsProfileOpenPreferences();
    await test.step("picking Kiritimati reshapes the clocks", async () => {
      await driver.settingsProfileSetTimezone("Pacific/Kiritimati");
      expect((await serverProfileLocale(session)).user_timezone).toBe("Pacific/Kiritimati");
    });
    await driver.settingsProfileOpenHome(seed.workspaceSlug);
    const kiritimatiClock = clockOf(await driver.settingsProfileGreetingClockText());
    await driver.settingsProfileOpenMemberProfile(seed.workspaceSlug, userId);
    expect(await driver.settingsProfileSidebarClockText()).toContain("Pacific/Kiritimati");

    await driver.settingsProfileOpenPreferences();
    await test.step("picking Midway shifts the clocks again", async () => {
      await driver.settingsProfileSetTimezone("Pacific/Midway");
      expect((await serverProfileLocale(session)).user_timezone).toBe("Pacific/Midway");
    });
    await driver.settingsProfileOpenHome(seed.workspaceSlug);
    // Twenty-five hours apart, so the hour always differs.
    expect(clockOf(await driver.settingsProfileGreetingClockText())).not.toBe(kiritimatiClock);
    await driver.settingsProfileOpenMemberProfile(seed.workspaceSlug, userId);
    expect(await driver.settingsProfileSidebarClockText()).toContain("Pacific/Midway");

    await test.step("restore the initial timezone", async () => {
      await driver.settingsProfileOpenPreferences();
      await driver.settingsProfileSetTimezone(initialTz);
      expect((await serverProfileLocale(session)).user_timezone).toBe(initialTz);
    });
  }
);

test(
  specTitle(ROWS_017, "timezone shifts stored dates rendered in your timezone"),
  { tag: specTags(ROWS_017) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const initialTz = (await serverProfileLocale(session)).user_timezone ?? "UTC";

    // Cycle days a few weeks out, so the range always lands on real dates.
    const toISODate = (plusDays: number): string => {
      const at = new Date();
      at.setUTCDate(at.getUTCDate() + plusDays);
      return at.toISOString().slice(0, 10);
    };
    // The app renders "MMM dd, yyyy" in the member timezone; mirror that
    // conversion from the stored ISO value for each expected reading.
    const renderIn = (iso: string, timeZone: string): string =>
      new Intl.DateTimeFormat("en-US", {
        month: "short",
        day: "2-digit",
        year: "numeric",
        timeZone,
      }).format(new Date(iso));

    const cycleId = await serverCreateCycle(
      seed.workspaceSlug,
      seed.projectId,
      `prof017 tz dates ${Date.now().toString(36)}`,
      toISODate(28),
      toISODate(35),
      session
    );
    try {
      // The stored timestamps stay fixed while the picks change; every
      // expected reading below derives from these two values.
      const stored = await serverCycleDetail(seed.workspaceSlug, seed.projectId, cycleId, session);
      expect(stored.startDate).not.toBeNull();
      expect(stored.endDate).not.toBeNull();
      const storedStart = stored.startDate!;
      const storedEnd = stored.endDate!;

      const readPair = async (timezone: string): Promise<{ start: string | null; end: string | null }> => {
        await driver.settingsProfileOpenPreferences();
        await driver.settingsProfileSetTimezone(timezone);
        expect((await serverProfileLocale(session)).user_timezone).toBe(timezone);
        await driver.openCyclePage(seed.workspaceSlug, seed.projectId, cycleId);
        return driver.settingsProfileCycleUserTimezoneDates();
      };

      let kiritimati: { start: string | null; end: string | null } = { start: null, end: null };
      await test.step("Kiritimati renders the stored dates Kiritimati-side", async () => {
        kiritimati = await readPair("Pacific/Kiritimati");
        expect(kiritimati).toEqual({
          start: renderIn(storedStart, "Pacific/Kiritimati"),
          end: renderIn(storedEnd, "Pacific/Kiritimati"),
        });
      });

      await test.step("Midway shifts both rendered dates", async () => {
        const midway = await readPair("Pacific/Midway");
        expect(midway).toEqual({
          start: renderIn(storedStart, "Pacific/Midway"),
          end: renderIn(storedEnd, "Pacific/Midway"),
        });
        // Twenty-five hours apart, so the same stored pair always reads
        // differently across the two picks.
        expect(midway).not.toEqual(kiritimati);
      });

      await test.step("restore the initial timezone", async () => {
        await driver.settingsProfileOpenPreferences();
        await driver.settingsProfileSetTimezone(initialTz);
        expect((await serverProfileLocale(session)).user_timezone).toBe(initialTz);
      });
    } finally {
      await serverDeleteCycle(seed.workspaceSlug, seed.projectId, cycleId, session);
    }
  }
);

test(
  specTitle(ROWS_017, "week start reorders calendar days"),
  { tag: specTags(ROWS_017) },
  async ({ driver, seed }) => {
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);
    const initialWeekStart = (await serverProfileLocale(session)).start_of_the_week ?? 0;

    const calendarFirstDay = async (): Promise<string[]> => {
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      await driver.layoutsSwitchTo("calendar");
      expect(await driver.layoutsCalendarVisible()).toBe(true);
      return driver.settingsProfileCalendarDayOrder();
    };

    await driver.settingsProfileOpenPreferences();
    await test.step("Monday first orders the week from Monday", async () => {
      await driver.settingsProfileSetWeekStart("Monday");
      expect((await serverProfileLocale(session)).start_of_the_week).toBe(1);
      const order = await calendarFirstDay();
      expect(order[0]).toBe("Mon");
      if (order.length === 7) {
        expect(order).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
      }
    });

    await driver.settingsProfileOpenPreferences();
    await test.step("Wednesday first orders the week from Wednesday", async () => {
      await driver.settingsProfileSetWeekStart("Wednesday");
      expect((await serverProfileLocale(session)).start_of_the_week).toBe(3);
      const order = await calendarFirstDay();
      expect(order[0]).toBe("Wed");
      if (order.length === 7) {
        expect(order).toEqual(["Wed", "Thu", "Fri", "Sat", "Sun", "Mon", "Tue"]);
      }
    });

    await test.step("restore the initial week start", async () => {
      const label = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][initialWeekStart]!;
      await driver.settingsProfileOpenPreferences();
      await driver.settingsProfileSetWeekStart(label);
      expect((await serverProfileLocale(session)).start_of_the_week).toBe(initialWeekStart);
      // Leave the issue layout as the suite found it.
      await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
      await driver.layoutsSwitchTo("list");
    });
  }
);

test(
  specTitle(ROWS_017, "language saves on pick while OSS ships English only"),
  { tag: specTags(ROWS_017) },
  async ({ driver, seed }) => {
    // The OSS build offers English alone, so no in-app pick can change the
    // value; stage French on the server first, then prove picking English
    // back saves through the UI with feedback. Setup precedes sign-in
    // because the profile GET is HTTP-cached for the running page.
    const session = await signInSession(seed.email, seed.password);
    expect((await serverPatchProfile(session, { language: "fr" })).ok).toBe(true);

    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    await driver.settingsProfileOpenPreferences();
    // French renders raw — the OSS build ships no French locale to label it.
    expect((await driver.settingsProfileLocaleValues()).language).toContain("fr");

    await test.step("picking English back saves through the UI", async () => {
      // The UI pick doubles as the restore: the scenario always ends on en.
      await driver.settingsProfileSetLanguage("English");
      expect((await serverProfileLocale(session)).language).toBe("en");
      expect((await driver.settingsProfileLocaleValues()).language).toContain("English");
    });
  }
);

test(
  specTitle(ROWS_018, "settings notifications tab renders the email-preferences entry"),
  { tag: specTags(ROWS_018) },
  async ({ driver, seed }) => {
    // Thin by design (PROF-018): the toggle depth lives in
    // notifications/email-preferences.spec.ts (NTF-024) and
    // notifications/email-delivery.spec.ts (NTF-025). This scenario only
    // proves the settings tab renders the email entry it links to.
    await test.step("sign in through the UI", async () => {
      await driver.signInWithPassword(seed.email, seed.password);
    });
    const session = await signInSession(seed.email, seed.password);

    await driver.notificationsOpenEmailPreferences();
    const ui = await driver.notificationsEmailPreferences();
    expect(Object.keys(ui).sort()).toEqual(
      ["comment", "issue_completed", "mention", "property_change", "state_change"].sort()
    );
    expect(ui).toEqual(await serverEmailPreferences(session));
  }
);
