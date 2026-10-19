// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preferences tab (parity/specs/profile.md §7, PROF-016/017). Four rows
// that save on pick: the interface theme (with the custom designer), the
// interface language, the timezone and the first day of the week. Theme
// and language go through their engines (instant re-skin and re-render);
// the other two persist through the shared mutations and confirm by toast.
// Every row reads the shared session caches, so a pick here reshapes the
// clocks, dates and calendars other screens render from the same sources.

import * as React from "react";

import { useLocale } from "../../../../core/i18n/index.js";
import { useMe, useProfile } from "../../../../core/session/index.js";
import {
  CustomThemeDesigner,
  readThemeBag,
  useTheme,
  type CustomPalette,
  type ThemeValue,
} from "../../../../core/theme/index.js";
import { useTimezones } from "../../../../shared/pickers/index.js";
import { useUpdateTimezone } from "../../../../shared/shell/preferences.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/toaster.js";
import { useSaveWeekStart, WEEK_START_FAILED_TITLE, WEEK_START_SAVED_TITLE } from "../api.js";
import {
  languageLabel,
  normalizeWeekStart,
  optionLabel,
  THEME_SELECT_OPTIONS,
  timezoneChoices,
  weekStartLabel,
  WEEK_START_OPTIONS,
} from "../lib/choices.js";
import { customPaletteOrDefault } from "../lib/customPalette.js";
import { reloadDocument } from "../lib/reload.js";
import { PreferencePicker } from "./PreferencePicker.js";
import { PreferenceRow } from "./PreferenceRow.js";

export const PREFERENCES_HEADING = "Preferences";
export const PREFERENCES_INTRO = "Shape how the app looks and which locale it speaks in.";

export const THEME_ROW_TITLE = "Theme";
export const THEME_ROW_DESCRIPTION = "Pick the palette every screen uses.";
export const LANGUAGE_ROW_TITLE = "Language";
export const LANGUAGE_ROW_DESCRIPTION = "Change the interface language.";
export const TIMEZONE_ROW_TITLE = "Timezone";
export const TIMEZONE_ROW_DESCRIPTION = "Show every date and clock in your own zone.";
export const WEEK_START_ROW_TITLE = "First day of the week";
export const WEEK_START_ROW_DESCRIPTION = "Order calendar columns from this day.";

export const LANGUAGE_SAVED_TITLE = "Language updated successfully.";
export const CUSTOM_SAVED_TITLE = "Custom theme saved";
export const CUSTOM_SAVED_MESSAGE = "Reloading to apply the saved colors.";

/** Whether the custom designer shows below the theme row. */
export function designerVisible(value: ThemeValue): boolean {
  return value === "custom";
}

export function PreferencesTab(): React.ReactElement {
  const theme = useTheme();
  const locale = useLocale();
  const profile = useProfile();
  const me = useMe();
  const updateTimezone = useUpdateTimezone();
  const saveWeekStart = useSaveWeekStart();
  const zonesQuery = useTimezones();
  const [preview, setPreview] = React.useState<CustomPalette | null>(null);

  // The applied palette, else the saved bag, else nothing for a member
  // who never customized. A custom pick persists only real saved colors;
  // the designer still starts a new member from usable defaults.
  const savedPalette = theme.custom ?? readThemeBag(profile.data?.theme).custom;
  const designerPalette = customPaletteOrDefault(savedPalette);
  const weekStart = normalizeWeekStart(profile.data?.start_of_the_week);
  const timezone = me.data?.user_timezone ?? "";

  const zones = React.useMemo(
    () => timezoneChoices(zonesQuery.data?.timezones ?? [], timezone),
    [zonesQuery.data, timezone]
  );

  function pickTheme(value: ThemeValue): void {
    // The engine carries the saved palette itself, so re-picking custom
    // shows the saved colors and a new member's pick stores only the name.
    void theme.setTheme(value);
  }

  function saveCustom(palette: CustomPalette): void {
    // Reloading races the profile write: wait for it to land, otherwise
    // the reload aborts the request and the colors never persist.
    void theme.setTheme("custom", palette).then((saved) => {
      if (!saved) return;
      notifySuccess(CUSTOM_SAVED_TITLE, CUSTOM_SAVED_MESSAGE);
      // The palette is applied already; reloading re-enters the app so
      // every cached surface boots from the stored colors.
      reloadDocument();
    });
  }

  function pickLanguage(tag: string): void {
    // The engine reports its own failures, so confirm only once the
    // profile write lands.
    void locale.setLanguage(tag).then((saved) => {
      if (saved) notifySuccess(LANGUAGE_SAVED_TITLE);
    });
  }

  return (
    <div data-testid="preferences-settings" className="flex flex-col gap-(--space-8)">
      <header className="flex flex-col gap-(--space-2)">
        <h1 className="text-h2 text-(--text)">{PREFERENCES_HEADING}</h1>
        <p className="text-body text-(--text-muted)">{PREFERENCES_INTRO}</p>
      </header>

      <div className="flex flex-col divide-y divide-(--border)">
        <div className="flex flex-col gap-(--space-4) pb-(--space-6)">
          <PreferenceRow
            title={THEME_ROW_TITLE}
            description={THEME_ROW_DESCRIPTION}
            control={
              <PreferencePicker
                label={THEME_ROW_TITLE}
                valueLabel={optionLabel(THEME_SELECT_OPTIONS, theme.value)}
                value={theme.value}
                options={THEME_SELECT_OPTIONS}
                onSelect={pickTheme}
                disabled={theme.isSaving}
              />
            }
          >
            {designerVisible(theme.value) ? (
              <div className="flex flex-col gap-(--space-3)">
                <CustomThemeDesigner
                  initial={designerPalette}
                  saving={theme.isSaving}
                  onSave={saveCustom}
                  onPreview={setPreview}
                />
                {/* Typing paints the swatches only; nothing reaches the
                    document, local storage or the profile until "Set theme". */}
                <div data-testid="custom-theme-preview" className="flex items-center gap-(--space-2)">
                  <span className="text-meta text-(--text-muted)">
                    {preview === null ? "Type both colors to see them here." : "Preview"}
                  </span>
                  {preview === null ? null : (
                    <>
                      <span
                        aria-label="Primary preview"
                        className="inline-block size-5 rounded-(--radius-control) border border-(--border)"
                        style={{ backgroundColor: preview.primary }}
                      />
                      <span
                        aria-label="Background preview"
                        className="inline-block size-5 rounded-(--radius-control) border border-(--border)"
                        style={{ backgroundColor: preview.background }}
                      />
                    </>
                  )}
                </div>
              </div>
            ) : null}
          </PreferenceRow>
        </div>

        <div className="py-(--space-6)">
          <PreferenceRow
            title={LANGUAGE_ROW_TITLE}
            description={LANGUAGE_ROW_DESCRIPTION}
            control={
              <PreferencePicker
                label={LANGUAGE_ROW_TITLE}
                valueLabel={locale.tag === "" ? "Not set" : languageLabel(locale.tag, locale.supported)}
                value={locale.tag}
                options={locale.supported.map((option) => ({ value: option.tag, label: option.label }))}
                onSelect={pickLanguage}
                disabled={locale.isSaving}
              />
            }
          />
        </div>

        <div className="py-(--space-6)">
          <PreferenceRow
            title={TIMEZONE_ROW_TITLE}
            description={TIMEZONE_ROW_DESCRIPTION}
            control={
              <PreferencePicker
                label={TIMEZONE_ROW_TITLE}
                valueLabel={zones.label}
                value={timezone}
                options={zones.options}
                onSelect={(value) => updateTimezone.mutate({ timezone: value })}
                disabled={updateTimezone.isPending}
                searchText={(option) => option.value}
              />
            }
          />
        </div>

        <div className="pt-(--space-6)">
          <PreferenceRow
            title={WEEK_START_ROW_TITLE}
            description={WEEK_START_ROW_DESCRIPTION}
            control={
              <PreferencePicker
                label={WEEK_START_ROW_TITLE}
                valueLabel={weekStartLabel(weekStart)}
                value={weekStart}
                options={WEEK_START_OPTIONS}
                onSelect={(value) =>
                  saveWeekStart.mutate(value, {
                    onSuccess: () => notifySuccess(WEEK_START_SAVED_TITLE),
                    onError: (error: Error) => notifyError(WEEK_START_FAILED_TITLE, error.message),
                  })
                }
                disabled={saveWeekStart.isPending}
              />
            }
          />
        </div>
      </div>
    </div>
  );
}
