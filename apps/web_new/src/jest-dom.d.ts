// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
