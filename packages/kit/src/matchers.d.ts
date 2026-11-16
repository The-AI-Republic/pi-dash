// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Vitest matcher types for the kit suite. Declared locally (rather than
// reused from jest-dom's own vitest entry) so the augmentation binds to
// this package's vitest: bare `vitest` resolves to v4 from inside
// jest-dom's directory, which neither merges with nor registers on the v3
// instance the suite runs against. Runtime registration happens
// explicitly in test-setup.ts; this file only carries types.
import type { TestingLibraryMatchers } from "@testing-library/jest-dom/matchers";

declare module "vitest" {
  interface Assertion<T = unknown> extends TestingLibraryMatchers<unknown, T> {}
  interface AsymmetricMatchersContaining extends TestingLibraryMatchers<unknown, unknown> {}
}
