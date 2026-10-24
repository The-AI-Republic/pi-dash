// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the spaces feature. Other features import only from here;
// each slice adds its own exports and keeps the other slices' lines.

export * from "./auth/index.js";
export * from "./lib/boardHead.js";
export * from "./lib/returnPath.js";
export { LandingGate, LandingGuidance, spaceLandingCapabilities } from "./components/LandingGate.js";
export type { LandingGateProps, LandingGuidanceProps } from "./components/LandingGate.js";
export { SpaceBoardHead } from "./components/SpaceBoardHead.js";
