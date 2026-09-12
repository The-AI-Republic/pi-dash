/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override: `/login` renders the same bare sign-in screen as `/`,
 * whatever an edition's web build puts there (a marketing-framed login page
 * linking to routes the desktop bundle does not ship — see ../../extended.ts).
 * Sign-out lands here.
 */

export { default, meta } from "../../../(home)/page";
