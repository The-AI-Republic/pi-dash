// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// jest-dom matchers for the vitest copy this package resolves.
// @testing-library/jest-dom's own augmentation anchors on whatever
// `vitest` resolves to from its types (currently v4 in the shared store),
// which is not the v3 copy these suites import — so the matchers land on
// the wrong Assertion interface. This file repeats the same augmentation
// anchored here, where `vitest` is v3.
import type { TestingLibraryMatchers } from "@testing-library/jest-dom/matchers";
import "vitest";

declare module "vitest" {
  // Mirrors @testing-library/jest-dom's own augmentation verbatim.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  interface Assertion<T = any> extends TestingLibraryMatchers<any, T> {}
}
