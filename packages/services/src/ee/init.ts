/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// Empty in OSS. Cloud (or other distributions) replace this file via
// build-time overlay to call `registerAxiosSetup(...)` from
// `../api.service` and install per-instance interceptors (eg. a 401 →
// refresh-token retry handler). Keep this file side-effect-only.
