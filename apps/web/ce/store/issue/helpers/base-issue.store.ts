/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TIssue } from "@pi-dash/types";
import { getIssueIds } from "@/store/issue/helpers/base-issues-utils";

export const workItemSortWithOrderByExtended = (array: TIssue[], _key?: string) => getIssueIds(array);
