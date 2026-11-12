// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  ALL_PROJECTS_LABEL,
  DEFAULT_EXPORT_PROVIDER,
  EXPORT_FAILED_MESSAGE,
  EXPORT_FAILED_TITLE,
  EXPORT_FORMAT_OPTIONS,
  EXPORT_SUCCESS_TITLE,
  exportRequestBody,
  ExportsForm,
  ExportsGuide,
  exportsKeys,
  formatLabel,
  parseExportForm,
  registerExportsGuide,
  selectExportProjectIds,
  selectExportSuccessMessage,
  selectPickedIdentifiers,
  selectProjectTriggerLabel,
  useCreateWorkspaceExport,
} from "./index.js";
import * as barrel from "./index.js";

describe("exports barrel", () => {
  it("exposes the form, model, keys and write hook", () => {
    expect(ExportsForm).toBeDefined();
    expect(exportsKeys).toBeDefined();
    expect(useCreateWorkspaceExport).toBeDefined();
    expect(selectExportSuccessMessage).toBeDefined();
    expect(EXPORT_SUCCESS_TITLE).toBe("Export successful");
    expect(EXPORT_FAILED_TITLE).toBe("Error");
    expect(EXPORT_FAILED_MESSAGE).toBe("Export was unsuccessful. Please try again.");
    expect(EXPORT_FORMAT_OPTIONS).toHaveLength(3);
    expect(DEFAULT_EXPORT_PROVIDER).toBe("csv");
    expect(ALL_PROJECTS_LABEL).toBe("All projects");
    expect(formatLabel).toBeDefined();
    expect(parseExportForm).toBeDefined();
    expect(exportRequestBody).toBeDefined();
    expect(selectExportProjectIds).toBeDefined();
    expect(selectPickedIdentifiers).toBeDefined();
    expect(selectProjectTriggerLabel).toBeDefined();
  });

  it("exposes the history section, hook, keys, and model", () => {
    expect(typeof barrel.ExportsHistory).toBe("function");
    expect(typeof barrel.useExportsHistoryPage).toBe("function");
    expect(typeof barrel.exportsHistoryPageQueryOptions).toBe("function");
    expect(typeof barrel.exportsKeys.history).toBe("function");
    expect(barrel.EXPORT_HISTORY_PAGE_SIZE).toBe(10);
    expect(barrel.EXPORT_HISTORY_FIRST_CURSOR).toBe("10:0:0");
    expect(typeof barrel.historyRowFacts).toBe("function");
    expect(typeof barrel.historyDownload).toBe("function");
    expect(typeof barrel.historyFormatLabel).toBe("function");
    expect(typeof barrel.historyInitiator).toBe("function");
    expect(typeof barrel.historyQuery).toBe("function");
    expect(typeof barrel.historyStartedLabel).toBe("function");
    expect(typeof barrel.historyStatus).toBe("function");
    expect(typeof barrel.isExpired).toBe("function");
    expect(typeof barrel.shouldPoll).toBe("function");
    expect(Object.keys(barrel.EXPORT_HISTORY_STATUS_TONES).sort()).toEqual([
      "completed",
      "failed",
      "processing",
      "queued",
      "unknown",
    ]);
  });

  it("exposes the settings guide and its seam registration", () => {
    expect(typeof ExportsGuide).toBe("function");
    expect(typeof registerExportsGuide).toBe("function");
    expect(barrel.ExportsGuide).toBe(ExportsGuide);
    expect(barrel.registerExportsGuide).toBe(registerExportsGuide);
  });
});
