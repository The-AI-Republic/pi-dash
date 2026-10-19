// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette preference entries (shell spec, SHELL-088/095-097). Four
// commands, each opening a sub-page: theme, timezone, first day of
// week and language. Theme and language picks go through their
// engines (immediate re-skin and re-render, no reload); the other two
// persist through the shared preference mutations.

import { useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { useLocale } from "../../../core/i18n/index.js";
import { useTheme } from "../../../core/theme/index.js";
import {
  filterPageOptions,
  useCommandPalette,
  type PalettePage,
  type PalettePageState,
  type PidashCommand,
} from "../../commands/index.js";
import {
  FIRST_DAY_CHOICES,
  THEME_CHOICES,
  timezonesQueryOptions,
  useUpdateFirstDay,
  useUpdateTimezone,
} from "../preferences.js";

/** Sub-page id behind "Change interface theme" (SHELL-088). */
export const THEME_PAGE_ID = "shell.theme-picker";
/** Sub-page id behind "Change timezone" (SHELL-088). */
export const TIMEZONE_PAGE_ID = "shell.timezone-picker";
/** Sub-page id behind "Change first day of week" (SHELL-088). */
export const FIRST_DAY_PAGE_ID = "shell.first-day-picker";
/** Sub-page id behind "Change interface language" (SHELL-088). */
export const LANGUAGE_PAGE_ID = "shell.language-picker";

/** The four preference entries, open to every signed-in user (SHELL-088). */
export function buildPreferenceCommands(): PidashCommand[] {
  return [
    {
      id: "shell.change-theme",
      title: "Change interface theme",
      section: "Preferences",
      page: THEME_PAGE_ID,
      run: () => undefined,
    },
    {
      id: "shell.change-timezone",
      title: "Change timezone",
      section: "Preferences",
      page: TIMEZONE_PAGE_ID,
      run: () => undefined,
    },
    {
      id: "shell.change-first-day",
      title: "Change first day of week",
      section: "Preferences",
      page: FIRST_DAY_PAGE_ID,
      run: () => undefined,
    },
    {
      id: "shell.change-language",
      title: "Change interface language",
      section: "Preferences",
      page: LANGUAGE_PAGE_ID,
      run: () => undefined,
    },
  ];
}

export function themePickerPage(): PalettePage {
  return {
    id: THEME_PAGE_ID,
    placeholder: "Choose a theme",
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const { setTheme } = useTheme();
      return {
        status: "ready",
        options: filterPageOptions(
          THEME_CHOICES.map((choice) => ({
            id: choice.value,
            title: choice.label,
            run: () => {
              closePalette();
              void setTheme(choice.value);
            },
          })),
          query
        ),
        emptyText: "No matching options.",
      };
    },
  };
}

export function timezonePickerPage(): PalettePage {
  return {
    id: TIMEZONE_PAGE_ID,
    placeholder: "Change timezone",
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const update = useUpdateTimezone();
      const zones = useQuery(timezonesQueryOptions(getAppClient()));
      if (zones.isPending) {
        return { status: "loading", options: [], emptyText: "No timezones found" };
      }
      const rows = zones.data?.timezones ?? [];
      return {
        status: "ready",
        options: filterPageOptions(
          rows.map((zone) => ({
            id: zone.value,
            title: zone.label,
            hint: zone.value,
            run: () => {
              closePalette();
              update.mutate({ timezone: zone.value });
            },
          })),
          query
        ),
        emptyText: "No timezones found",
      };
    },
  };
}

export function firstDayPickerPage(): PalettePage {
  return {
    id: FIRST_DAY_PAGE_ID,
    placeholder: "Choose the first day of the week",
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const update = useUpdateFirstDay();
      return {
        status: "ready",
        options: filterPageOptions(
          FIRST_DAY_CHOICES.map((choice) => ({
            id: String(choice.value),
            title: choice.label,
            run: () => {
              closePalette();
              update.mutate({ startOfWeek: choice.value });
            },
          })),
          query
        ),
        emptyText: "No matching options.",
      };
    },
  };
}

export function languagePickerPage(): PalettePage {
  return {
    id: LANGUAGE_PAGE_ID,
    placeholder: "Choose a language",
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const { setLanguage, supported } = useLocale();
      return {
        status: "ready",
        options: filterPageOptions(
          supported.map((choice) => ({
            id: choice.tag,
            title: choice.label,
            run: () => {
              closePalette();
              void setLanguage(choice.tag);
            },
          })),
          query
        ),
        emptyText: "No matching options.",
      };
    },
  };
}
