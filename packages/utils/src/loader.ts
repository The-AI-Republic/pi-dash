/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TLoader } from "@pi-dash/types";

// checks if a loader has finished initialization
export const isLoaderReady = (loader: TLoader | undefined) => loader !== "init-loader";
