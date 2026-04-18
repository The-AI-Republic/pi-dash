/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { LucideIcon } from "lucide-react";
import { ArrowUpToLine, Building, CreditCard, Users, Webhook } from "lucide-react";
// apple pi dash imports
import type { ISvgIcons } from "@apple-pi-dash/propel/icons";
import type { TWorkspaceSettingsTabs } from "@apple-pi-dash/types";

export const WORKSPACE_SETTINGS_ICONS: Record<TWorkspaceSettingsTabs, LucideIcon | React.FC<ISvgIcons>> = {
  general: Building,
  members: Users,
  export: ArrowUpToLine,
  "billing-and-plans": CreditCard,
  webhooks: Webhook,
};
