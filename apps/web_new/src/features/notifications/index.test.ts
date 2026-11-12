// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feature's public surface: other features and routes import only from
// this barrel, so every name it promises must resolve here.
import { describe, expect, it } from "vitest";

import {
  applyRowToPage,
  CUSTOM_SNOOZE_CANCEL,
  CUSTOM_SNOOZE_ENTRY,
  CUSTOM_SNOOZE_SUBMIT,
  composeResumeInstant,
  CustomSnoozeDialog,
  DATE_PLACEHOLDER,
  DATE_SECTION_HEADING,
  dayIsFullyElapsed,
  earliestSnoozeDay,
  formatResumeLabel,
  InboxShell,
  InboxView,
  MARKED_ARCHIVED_TITLE,
  MARKED_READ_TITLE,
  MARKED_UNREAD_TITLE,
  mergeWrittenRow,
  NO_SLOTS_LEFT,
  notificationKeys,
  notificationListQueryFromKey,
  NotificationCard,
  offeredSlots,
  pickedDayLabel,
  presetResumeInstant,
  REMOVE_SNOOZE_ENTRY,
  rowBelongsToStream,
  SLOTS_PER_MERIDIEM,
  slotLabels,
  slotTo24Hour,
  slotToMinutes,
  SNOOZED_TITLE,
  SnoozeMenu,
  SNOOZE_PRESETS,
  snoozePresetByLabel,
  TIME_PLACEHOLDER,
  TIME_SECTION_HEADING,
  UNARCHIVED_TITLE,
  UNSNOOZED_TITLE,
  useSnoozeNotification,
  useToggleNotificationArchive,
  useToggleNotificationRead,
} from "./index.js";

describe("notifications barrel", () => {
  it("re-exports every name the inbox route and later slices consume", () => {
    for (const hook of [useToggleNotificationRead, useToggleNotificationArchive, useSnoozeNotification]) {
      expect(typeof hook).toBe("function");
    }
    for (const component of [CustomSnoozeDialog, InboxShell, InboxView, NotificationCard, SnoozeMenu]) {
      expect(component).toBeDefined();
    }
    for (const fn of [
      applyRowToPage,
      composeResumeInstant,
      dayIsFullyElapsed,
      earliestSnoozeDay,
      formatResumeLabel,
      mergeWrittenRow,
      notificationListQueryFromKey,
      offeredSlots,
      pickedDayLabel,
      presetResumeInstant,
      rowBelongsToStream,
      slotTo24Hour,
      slotToMinutes,
      snoozePresetByLabel,
    ]) {
      expect(typeof fn).toBe("function");
    }
    expect(SNOOZE_PRESETS.length).toBe(5);
    expect(SLOTS_PER_MERIDIEM).toBe(24);
    expect(slotLabels()).toHaveLength(SLOTS_PER_MERIDIEM);
    expect(notificationKeys.all("acme")).toBeDefined();
    for (const text of [
      CUSTOM_SNOOZE_CANCEL,
      CUSTOM_SNOOZE_ENTRY,
      CUSTOM_SNOOZE_SUBMIT,
      DATE_PLACEHOLDER,
      DATE_SECTION_HEADING,
      MARKED_ARCHIVED_TITLE,
      MARKED_READ_TITLE,
      MARKED_UNREAD_TITLE,
      NO_SLOTS_LEFT,
      REMOVE_SNOOZE_ENTRY,
      SNOOZED_TITLE,
      TIME_PLACEHOLDER,
      TIME_SECTION_HEADING,
      UNARCHIVED_TITLE,
      UNSNOOZED_TITLE,
    ]) {
      expect(text.length).toBeGreaterThan(0);
    }
  });
});
