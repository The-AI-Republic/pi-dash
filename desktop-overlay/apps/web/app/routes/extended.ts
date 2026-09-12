/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop route extensions.
 *
 * Editions use this seam to add web-only routes (public marketing pages,
 * downloads, …). The desktop bundle ships none of them: desktop-overlay is
 * applied after every edition overlay, so this file wins and leaves the
 * table empty, and those paths fall through to the 404 catch-all.
 * Desktop-only routes (local runner settings, etc.) get added here.
 */

import type { RouteConfigEntry } from "@react-router/dev/routes";

export const extendedRoutes: RouteConfigEntry[] = [];
