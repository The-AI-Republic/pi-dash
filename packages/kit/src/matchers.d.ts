// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
