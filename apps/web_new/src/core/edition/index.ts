// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
