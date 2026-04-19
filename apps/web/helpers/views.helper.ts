/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { LucideIcon } from "lucide-react";
import { VIEW_ACCESS_SPECIFIERS as VIEW_ACCESS_SPECIFIERS_CONSTANTS } from "@pi-dash/constants";
import { GlobeIcon, LockIcon } from "@pi-dash/propel/icons";

import type { ISvgIcons } from "@pi-dash/propel/icons";
import { EViewAccess } from "@pi-dash/types";

const VIEW_ACCESS_ICONS = {
  [EViewAccess.PUBLIC]: GlobeIcon,
  [EViewAccess.PRIVATE]: LockIcon,
};

export const VIEW_ACCESS_SPECIFIERS: {
  key: EViewAccess;
  i18n_label: string;
  icon: LucideIcon | React.FC<ISvgIcons>;
}[] = VIEW_ACCESS_SPECIFIERS_CONSTANTS.map((option) => ({
  ...option,
  icon: VIEW_ACCESS_ICONS[option.key as keyof typeof VIEW_ACCESS_ICONS],
}));
