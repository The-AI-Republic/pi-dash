// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preferences tab (parity/specs/profile.md §7, PROF-016/017): theme,
// language, timezone and week start save on pick; the custom designer
// previews live and persists then reloads.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  theme: {
    value: "dark" as string,
    resolved: "dark",
    custom: undefined as { primary: string; background: string; darkPalette: boolean } | undefined,
    isSaving: false,
    setTheme: vi.fn(),
  },
  locale: {
    tag: "en",
    supported: [{ tag: "en", label: "English" }] as Array<{ tag: string; label: string }>,
    isSaving: false,
    setLanguage: vi.fn(),
  },
  profile: undefined as Record<string, unknown> | undefined,
  me: undefined as Record<string, unknown> | undefined,
  zones: [] as Array<{ value: string; label: string; utc_offset: string; gmt_offset: string }>,
  zonesLoaded: true,
  updateTimezone: { mutate: vi.fn(), isPending: false },
  saveWeekStart: { mutate: vi.fn(), isPending: false },
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
  reload: vi.fn(),
}));

vi.mock("../../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/session/index.js")>()),
  useProfile: () => ({ data: harness.profile }),
  useMe: () => ({ data: harness.me }),
}));

vi.mock("../../../../core/i18n/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/i18n/index.js")>()),
  useLocale: () => harness.locale,
}));

vi.mock("../../../../core/theme/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../core/theme/index.js")>();
  return { ...original, useTheme: () => harness.theme };
});

vi.mock("../../../../shared/pickers/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/pickers/index.js")>()),
  useTimezones: () => ({ data: harness.zonesLoaded ? { timezones: harness.zones } : undefined }),
}));

vi.mock("../../../../shared/shell/preferences.js", () => ({
  useUpdateTimezone: () => harness.updateTimezone,
}));

vi.mock("../../../../shared/shell/toaster.js", () => ({
  notifySuccess: harness.notifySuccess,
  notifyError: harness.notifyError,
}));

vi.mock("../api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api.js")>()),
  useSaveWeekStart: () => harness.saveWeekStart,
}));

vi.mock("../lib/reload.js", () => ({ reloadDocument: harness.reload }));

import {
  CUSTOM_SAVED_MESSAGE,
  CUSTOM_SAVED_TITLE,
  LANGUAGE_SAVED_TITLE,
  PREFERENCES_HEADING,
  PreferencesTab,
} from "./PreferencesTab.js";
import { WEEK_START_FAILED_TITLE, WEEK_START_SAVED_TITLE } from "../api.js";

const PROFILE = {
  theme: { theme: "dark" },
  start_of_the_week: 1,
};

const ME = { user_timezone: "America/Los_Angeles" };

beforeEach(() => {
  harness.theme = {
    value: "dark",
    resolved: "dark",
    custom: undefined,
    isSaving: false,
    setTheme: vi.fn(async () => true),
  };
  harness.locale = {
    tag: "en",
    supported: [{ tag: "en", label: "English" }],
    isSaving: false,
    setLanguage: vi.fn(async () => true),
  };
  harness.profile = PROFILE;
  harness.me = ME;
  harness.zonesLoaded = true;
  harness.zones = [
    { value: "America/Los_Angeles", label: "Los Angeles", utc_offset: "UTC-08:00", gmt_offset: "-8" },
    { value: "Europe/Paris", label: "Paris", utc_offset: "UTC+01:00", gmt_offset: "1" },
  ];
  harness.updateTimezone = { mutate: vi.fn(), isPending: false };
  harness.saveWeekStart = { mutate: vi.fn(), isPending: false };
  harness.notifySuccess.mockClear();
  harness.notifyError.mockClear();
  harness.reload.mockClear();
});

function trigger(name: string): HTMLElement {
  return screen.getByRole("button", { name });
}

describe("PreferencesTab", () => {
  it("renders the four rows with their stored labels", () => {
    render(<PreferencesTab />);
    expect(screen.getByRole("heading", { level: 1, name: PREFERENCES_HEADING })).toBeInTheDocument();
    for (const title of ["Theme", "Language", "Timezone", "First day of the week"]) {
      expect(screen.getByRole("heading", { level: 2, name: title })).toBeInTheDocument();
    }
    expect(trigger("Theme")).toHaveTextContent("Dark");
    expect(trigger("Language")).toHaveTextContent("English");
    expect(trigger("Timezone")).toHaveTextContent("Los Angeles");
    expect(trigger("First day of the week")).toHaveTextContent("Monday");
    expect(screen.queryByRole("button", { name: "Set theme" })).not.toBeInTheDocument();
  });

  it("applies a stock theme the moment it is picked", async () => {
    render(<PreferencesTab />);
    fireEvent.click(trigger("Theme"));
    fireEvent.click(await screen.findByRole("option", { name: "Light" }));
    expect(harness.theme.setTheme).toHaveBeenCalledWith("light");
    expect(harness.theme.setTheme).toHaveBeenCalledTimes(1);
  });

  it("picks custom through the engine, which re-applies the saved palette", async () => {
    harness.theme.value = "light";
    harness.theme.custom = { primary: "#102030", background: "#405060", darkPalette: true };
    render(<PreferencesTab />);
    fireEvent.click(trigger("Theme"));
    fireEvent.click(await screen.findByRole("option", { name: "Custom theme" }));
    // The engine carries the saved colors; the tab never sends a palette
    // of its own, so a member without saved colors stores only the name.
    expect(harness.theme.setTheme).toHaveBeenCalledWith("custom");
  });

  it("starts the designer from the profile bag when the engine has no palette", () => {
    harness.theme.value = "custom";
    harness.theme.custom = undefined;
    harness.profile = { theme: { theme: "custom", primary: "#aabbcc", background: "#ddeeff", darkPalette: false } };
    render(<PreferencesTab />);
    expect(screen.getByLabelText("Primary color")).toHaveValue("#aabbcc");
    expect(screen.getByLabelText("Background color")).toHaveValue("#ddeeff");
  });

  it("starts the designer from the defaults without persisting them", () => {
    harness.theme.value = "custom";
    harness.theme.custom = undefined;
    render(<PreferencesTab />);
    // The fallback pair is a starting point for the form, not a save:
    // opening the tab must leave the profile bag alone.
    expect(screen.getByLabelText("Primary color")).toHaveValue("#5e6ad2");
    expect(screen.getByLabelText("Background color")).toHaveValue("#ffffff");
    expect(harness.theme.setTheme).not.toHaveBeenCalled();
  });

  it("previews a typed pair in the swatches without saving anything", () => {
    harness.theme.value = "custom";
    harness.theme.custom = { primary: "#102030", background: "#405060", darkPalette: false };
    render(<PreferencesTab />);
    fireEvent.change(screen.getByLabelText("Primary color"), { target: { value: "#AABBCC" } });
    const preview = screen.getByTestId("custom-theme-preview");
    expect(within(preview).getByLabelText("Primary preview")).toHaveStyle({ backgroundColor: "#aabbcc" });
    expect(within(preview).getByLabelText("Background preview")).toHaveStyle({ backgroundColor: "#405060" });
    // Typing is a preview only: nothing reaches the engine (and so neither
    // the profile nor local storage) until "Set theme".
    expect(harness.theme.setTheme).not.toHaveBeenCalled();
  });

  it("saves the custom palette, then confirms and reloads once the write lands", async () => {
    harness.theme.value = "custom";
    harness.theme.custom = { primary: "#102030", background: "#405060", darkPalette: true };
    let settle: (saved: boolean) => void = () => undefined;
    harness.theme.setTheme.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          settle = resolve;
        })
    );
    render(<PreferencesTab />);
    fireEvent.change(screen.getByLabelText("Primary color"), { target: { value: "#AABBCC" } });
    fireEvent.click(screen.getByRole("button", { name: "Set theme" }));
    expect(harness.theme.setTheme).toHaveBeenCalledWith("custom", {
      primary: "#aabbcc",
      background: "#405060",
      darkPalette: true,
    });
    // The reload would abort the profile write, so nothing happens yet.
    expect(harness.notifySuccess).not.toHaveBeenCalled();
    expect(harness.reload).not.toHaveBeenCalled();
    settle(true);
    await waitFor(() => expect(harness.reload).toHaveBeenCalledTimes(1));
    expect(harness.notifySuccess).toHaveBeenCalledWith(CUSTOM_SAVED_TITLE, CUSTOM_SAVED_MESSAGE);
  });

  it("keeps the page on a rejected custom save", async () => {
    harness.theme.value = "custom";
    harness.theme.custom = { primary: "#102030", background: "#405060", darkPalette: true };
    harness.theme.setTheme.mockResolvedValue(false);
    render(<PreferencesTab />);
    fireEvent.click(screen.getByRole("button", { name: "Set theme" }));
    await waitFor(() => expect(harness.theme.setTheme).toHaveBeenCalled());
    expect(harness.notifySuccess).not.toHaveBeenCalled();
    expect(harness.reload).not.toHaveBeenCalled();
  });

  it("disables the theme row while a write is in flight", () => {
    harness.theme.isSaving = true;
    render(<PreferencesTab />);
    expect(trigger("Theme")).toBeDisabled();
    fireEvent.click(trigger("Theme"));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("switches language and confirms once the write lands", async () => {
    harness.locale.tag = "fr";
    harness.locale.supported = [
      { tag: "en", label: "English" },
      { tag: "fr", label: "Français" },
    ];
    let settle: (saved: boolean) => void = () => undefined;
    harness.locale.setLanguage.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          settle = resolve;
        })
    );
    render(<PreferencesTab />);
    expect(trigger("Language")).toHaveTextContent("Français");
    fireEvent.click(trigger("Language"));
    fireEvent.click(screen.getByRole("option", { name: "English" }));
    expect(harness.locale.setLanguage).toHaveBeenCalledWith("en");
    expect(harness.notifySuccess).not.toHaveBeenCalled();
    settle(true);
    await waitFor(() => expect(harness.notifySuccess).toHaveBeenCalledWith(LANGUAGE_SAVED_TITLE));
  });

  it("leaves the error report alone when the language write is refused", async () => {
    harness.locale.tag = "fr";
    harness.locale.supported = [
      { tag: "en", label: "English" },
      { tag: "fr", label: "Français" },
    ];
    harness.locale.setLanguage.mockResolvedValue(false);
    render(<PreferencesTab />);
    fireEvent.click(trigger("Language"));
    fireEvent.click(screen.getByRole("option", { name: "English" }));
    await waitFor(() => expect(harness.locale.setLanguage).toHaveBeenCalledWith("en"));
    expect(harness.notifySuccess).not.toHaveBeenCalled();
    expect(harness.notifyError).not.toHaveBeenCalled();
  });

  it("shows the raw tag when this build offers no label", () => {
    harness.locale.tag = "de";
    harness.locale.supported = [
      { tag: "en", label: "English" },
      { tag: "de", label: "Deutsch" },
    ].filter((option) => option.tag !== "de");
    render(<PreferencesTab />);
    expect(trigger("Language")).toHaveTextContent("de");
  });

  it("names the stored profile language over the engine's local tag", () => {
    // The engine can boot from a stored local tag that outranks the server
    // value; the row still reports what the profile holds.
    harness.locale.tag = "en";
    harness.profile = { ...PROFILE, language: "fr" };
    render(<PreferencesTab />);
    expect(trigger("Language")).toHaveTextContent("fr");
    expect(trigger("Language")).not.toHaveTextContent("English");
  });

  it("falls back to the engine tag until the profile lands", () => {
    harness.locale.tag = "en";
    harness.profile = undefined;
    render(<PreferencesTab />);
    expect(trigger("Language")).toHaveTextContent("English");
  });

  it("names an unset language and an unset timezone", () => {
    harness.locale.tag = "";
    harness.me = { user_timezone: "" };
    const { unmount } = render(<PreferencesTab />);
    expect(trigger("Language")).toHaveTextContent("Not set");
    expect(trigger("Timezone")).toHaveTextContent("Not set");
    unmount();
    // The member row has not landed yet: both rows read their empty value.
    harness.me = undefined;
    harness.profile = undefined;
    const second = render(<PreferencesTab />);
    expect(trigger("Timezone")).toHaveTextContent("Not set");
    second.unmount();
    // The zone catalog has not landed yet: only the stored zone shows.
    harness.zonesLoaded = false;
    harness.me = { user_timezone: "Pacific/Auckland" };
    render(<PreferencesTab />);
    expect(trigger("Timezone")).toHaveTextContent("Pacific/Auckland");
  });

  it("disables the language row while a write is in flight", () => {
    harness.locale.isSaving = true;
    render(<PreferencesTab />);
    expect(trigger("Language")).toBeDisabled();
  });

  it("writes the picked timezone through the shared mutation", async () => {
    render(<PreferencesTab />);
    fireEvent.click(trigger("Timezone"));
    const listbox = await screen.findByRole("listbox", { name: "Timezone options" });
    expect(within(listbox).getAllByRole("option")).toHaveLength(2);
    fireEvent.click(screen.getByRole("option", { name: "Paris" }));
    expect(harness.updateTimezone.mutate).toHaveBeenCalledWith({ timezone: "Europe/Paris" });
  });

  it("keeps a stored zone the catalog does not list and names an unset zone", () => {
    harness.me = { user_timezone: "Pacific/Auckland" };
    const { unmount } = render(<PreferencesTab />);
    expect(trigger("Timezone")).toHaveTextContent("Pacific/Auckland");
    fireEvent.click(trigger("Timezone"));
    expect(screen.getByRole("option", { name: "Pacific/Auckland" })).toBeInTheDocument();
    unmount();

    harness.me = { user_timezone: "" };
    render(<PreferencesTab />);
    expect(trigger("Timezone")).toHaveTextContent("Not set");
  });

  it("disables the timezone row while the write is in flight", () => {
    harness.updateTimezone.isPending = true;
    render(<PreferencesTab />);
    expect(trigger("Timezone")).toBeDisabled();
  });

  it("writes the picked week start and confirms on success", () => {
    harness.saveWeekStart.mutate = vi.fn((_value: number, options: { onSuccess: () => void }) => {
      options.onSuccess();
    });
    render(<PreferencesTab />);
    fireEvent.click(trigger("First day of the week"));
    fireEvent.click(screen.getByRole("option", { name: "Saturday" }));
    expect(harness.saveWeekStart.mutate).toHaveBeenCalledWith(6, expect.any(Object));
    expect(harness.notifySuccess).toHaveBeenCalledWith(WEEK_START_SAVED_TITLE);
  });

  it("reports a failed week-start write", () => {
    harness.saveWeekStart.mutate = vi.fn((_value: number, options: { onError: (error: Error) => void }) => {
      options.onError(new Error("boom"));
    });
    render(<PreferencesTab />);
    fireEvent.click(trigger("First day of the week"));
    fireEvent.click(screen.getByRole("option", { name: "Tuesday" }));
    expect(harness.notifyError).toHaveBeenCalledWith(WEEK_START_FAILED_TITLE, "boom");
  });

  it("normalizes an out-of-range stored week start to Sunday", () => {
    harness.profile = { theme: { theme: "dark" }, start_of_the_week: 12 };
    render(<PreferencesTab />);
    expect(trigger("First day of the week")).toHaveTextContent("Sunday");
  });

  it("disables the week-start row while the write is in flight", () => {
    harness.saveWeekStart.isPending = true;
    render(<PreferencesTab />);
    expect(trigger("First day of the week")).toBeDisabled();
  });

  it("searches the timezone list by zone value", async () => {
    const user = userEvent.setup();
    render(<PreferencesTab />);
    fireEvent.click(trigger("Timezone"));
    await user.type(screen.getByRole("combobox", { name: "Search" }), "paris");
    expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(1);
    await user.clear(screen.getByRole("combobox", { name: "Search" }));
    await waitFor(() => expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(2));
  });
});
