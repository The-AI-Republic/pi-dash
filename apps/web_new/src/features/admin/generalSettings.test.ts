// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { InstanceAdminList, InstanceBootstrap, InstanceDetails } from "@pidash/api-client";

import {
  firstAdminEmail,
  GENERAL_NAME_REQUIRED_MESSAGE,
  GENERAL_SAVE_BUSY_LABEL,
  GENERAL_SAVE_FAILURE_LOG,
  GENERAL_SAVE_IDLE_LABEL,
  GENERAL_SAVE_SUCCESS_MESSAGE,
  GENERAL_SAVE_SUCCESS_TITLE,
  GENERAL_TELEMETRY_DOC_URL,
  isGeneralNameValid,
  mergeGeneralSettingsIntoBootstrap,
  resolveTelemetryPrefill,
} from "./generalSettings.js";

function adminRow(email: string): InstanceAdminList[number] {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    user: "44444444-4444-4444-8444-444444444444",
    instance: "55555555-5555-4555-8555-555555555555",
    role: 1,
    is_verified: true,
    user_detail: {
      id: "44444444-4444-4444-8444-444444444444",
      first_name: "Ada",
      last_name: "Admin",
      avatar: "",
      avatar_url: null,
      is_bot: false,
      display_name: "Ada Admin",
      email,
      last_login_medium: "password",
    },
  };
}

function storedRow(overrides: Partial<InstanceDetails> = {}): InstanceDetails {
  return {
    id: "55555555-5555-4555-8555-555555555555",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    instance_name: "Pi Dash",
    whitelist_emails: null,
    instance_id: "instance-1",
    current_version: "1.0.0",
    latest_version: null,
    edition: "oss",
    domain: "example.com",
    last_checked_at: "2026-01-01T00:00:00Z",
    namespace: null,
    is_telemetry_enabled: true,
    is_support_required: false,
    is_setup_done: true,
    is_signup_screen_visited: true,
    is_verified: false,
    is_test: false,
    is_current_version_deprecated: false,
    ...overrides,
  };
}

describe("general settings copy", () => {
  it("pins the oracle-asserted strings", () => {
    expect(GENERAL_SAVE_IDLE_LABEL).toBe("Save changes");
    expect(GENERAL_SAVE_BUSY_LABEL).toBe("Saving");
    expect(GENERAL_SAVE_SUCCESS_TITLE).toBe("Success");
    expect(GENERAL_SAVE_SUCCESS_MESSAGE).toBe("Settings updated successfully");
    expect(GENERAL_SAVE_FAILURE_LOG).toBe("Error updating the instance info");
    expect(GENERAL_TELEMETRY_DOC_URL).toBe("https://github.com/The-AI-Republic/pi-dash#readme");
  });

  it("carries a visible required message for the NEWFRONT-475 intent", () => {
    expect(GENERAL_NAME_REQUIRED_MESSAGE.length).toBeGreaterThan(0);
  });
});

describe("isGeneralNameValid", () => {
  it("accepts a non-blank name", () => {
    expect(isGeneralNameValid("Pi Dash")).toBe(true);
  });

  it("rejects an empty name", () => {
    expect(isGeneralNameValid("")).toBe(false);
  });

  it("rejects a whitespace-only name", () => {
    expect(isGeneralNameValid("   ")).toBe(false);
  });
});

describe("firstAdminEmail", () => {
  it("reads the first entry's nested user email", () => {
    expect(firstAdminEmail([adminRow("ada@example.com"), adminRow("grace@example.com")])).toBe("ada@example.com");
  });

  it("reads blank until the list resolves", () => {
    expect(firstAdminEmail(undefined)).toBe("");
  });

  it("reads blank when the list holds no admins", () => {
    expect(firstAdminEmail([])).toBe("");
  });
});

describe("resolveTelemetryPrefill", () => {
  it("keeps the stored flag either way", () => {
    expect(resolveTelemetryPrefill(true)).toBe(true);
    expect(resolveTelemetryPrefill(false)).toBe(false);
  });

  it("reads an unset flag as off (unreachable: the flag is a non-nullable boolean)", () => {
    expect(resolveTelemetryPrefill(null)).toBe(false);
    expect(resolveTelemetryPrefill(undefined)).toBe(false);
  });
});

describe("mergeGeneralSettingsIntoBootstrap", () => {
  it("takes the saved row and keeps the cached workspace flag", () => {
    const current: InstanceBootstrap = {
      config: { enable_signup: true } as InstanceBootstrap["config"],
      instance: { ...storedRow(), workspaces_exist: true },
    };
    const updated = storedRow({ instance_name: "Renamed", is_telemetry_enabled: false });
    const merged = mergeGeneralSettingsIntoBootstrap(current, updated);
    expect(merged.instance.instance_name).toBe("Renamed");
    expect(merged.instance.is_telemetry_enabled).toBe(false);
    expect(merged.instance.workspaces_exist).toBe(true);
    expect(merged.config).toBe(current.config);
  });
});
