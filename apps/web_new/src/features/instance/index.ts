// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Startup-gate feature barrel (parity/specs/workspace-settings.md §34-35,
// WSET-034/035).

export { ADMIN_ENTRY_HREF, PRODUCT_MARK_SRC, PRODUCT_NAME } from "./brand.js";
export { InstanceAppGate } from "./InstanceAppGate.js";
export type { InstanceAppGateProps } from "./InstanceAppGate.js";
export {
  MAINTENANCE_ARM_DARK,
  MAINTENANCE_ARM_LIGHT,
  MAINTENANCE_HEADING,
  MAINTENANCE_SUPPORT_HREF,
  MaintenanceScreen,
  maintenanceIllustration,
} from "./MaintenanceScreen.js";
export { NOT_READY_BODY, NOT_READY_HEADING, NotReadyScreen } from "./NotReadyScreen.js";
export { appInstanceQueryOptions, instanceAppKeys, persistAppCache, useAppInstanceBootstrap } from "./queries.js";
export type { AppInstanceBootstrap } from "./queries.js";
export { decideAppInstanceGate } from "./status.js";
export type { AppInstanceBranch, AppInstanceGateInput } from "./status.js";
export { markInstanceBootScheme } from "./theme.js";
