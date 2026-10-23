// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_MODULES_LAYOUT,
  formatModuleDay,
  formatModuleRange,
  invalidateAfterModuleWrite,
  invalidateArchivedModuleReads,
  invalidateModuleLists,
  invalidateModuleReads,
  invalidateWorkspaceModules,
  MODULES_CRUMB,
  MODULES_LAYOUTS,
  moduleDateText,
  moduleDetailHref,
  moduleKeys,
  modulesFeatureHref,
  modulesListHref,
  modulesListQueryOptions,
  modulesTitle,
  ModuleBoardCard,
  ModuleLayoutIcon,
  ModuleListRow,
  ModulesFeatureGate,
  ModulesLayoutSwitcher,
  ModulesListBody,
  ModulesListHeader,
  ModulesListScreen,
  ModulesListSkeleton,
  ModuleTimelineRow,
  parseModuleDay,
  resolveModulesListBody,
  sortModulesByName,
  userZoneDiffers,
} from "./index.js";

describe("modules barrel", () => {
  it("re-exports the freshness helpers later slices call", () => {
    expect(moduleKeys).toBeDefined();
    expect(modulesListQueryOptions).toBeDefined();
    expect(invalidateModuleLists).toBeDefined();
    expect(invalidateModuleReads).toBeDefined();
    expect(invalidateArchivedModuleReads).toBeDefined();
    expect(invalidateWorkspaceModules).toBeDefined();
    expect(invalidateAfterModuleWrite).toBeDefined();
  });

  it("re-exports the list shell and its states", () => {
    expect(MODULES_CRUMB).toBe("Modules");
    expect(modulesTitle).toBeDefined();
    expect(modulesListHref).toBeDefined();
    expect(moduleDetailHref).toBeDefined();
    expect(modulesFeatureHref).toBeDefined();
    expect(resolveModulesListBody).toBeDefined();
    expect(sortModulesByName).toBeDefined();
    expect(ModulesListScreen).toBeDefined();
    expect(ModulesListHeader).toBeDefined();
    expect(ModulesFeatureGate).toBeDefined();
    expect(ModulesListSkeleton).toBeDefined();
    expect(ModulesListBody).toBeDefined();
    expect(ModulesLayoutSwitcher).toBeDefined();
    expect(ModuleLayoutIcon).toBeDefined();
    expect(ModuleListRow).toBeDefined();
    expect(ModuleBoardCard).toBeDefined();
    expect(ModuleTimelineRow).toBeDefined();
  });

  it("re-exports the layout vocabulary and the shared date helpers", () => {
    expect(MODULES_LAYOUTS).toEqual(["list", "board", "timeline"]);
    expect(DEFAULT_MODULES_LAYOUT).toBe("list");
    expect(parseModuleDay).toBeDefined();
    expect(formatModuleDay).toBeDefined();
    expect(formatModuleRange).toBeDefined();
    expect(moduleDateText).toBeDefined();
    expect(userZoneDiffers).toBeDefined();
  });
});
