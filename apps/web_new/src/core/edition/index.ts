// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Features import { edition } from here. The value comes from the single
// @pidash/edition alias (oss.ts in this repo), which the cloud build
// replaces with its own module implementing the same interface.

export type {
  AuthProviderExtension,
  Edition,
  RouteExtension,
  SettingsSection,
  SidebarItem,
  SlotComponents,
} from "./types.js";
export { edition } from "@pidash/edition";
