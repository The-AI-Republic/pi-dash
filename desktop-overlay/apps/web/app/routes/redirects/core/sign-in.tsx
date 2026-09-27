/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override: `/sign-in` renders the desktop sign-in screen so the
 * `?error=` query from a failed server `desktop-exchange` or a denied
 * identity-provider flow (main.rs deep-link handler) is displayed. The web
 * build's version redirects to `/` and drops the query.
 */

export { default, meta } from "../../../(home)/page";
