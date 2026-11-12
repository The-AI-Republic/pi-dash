// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Startup-gate decision, shared with the public space subtree. The pure
// function lives in `core/instance` so a feature can consume it without
// reaching into another feature's module; this module keeps the
// feature-local import path stable.

export { decideAppInstanceGate } from "../../core/instance/status.js";
export type { AppInstanceBranch, AppInstanceGateInput } from "../../core/instance/status.js";
