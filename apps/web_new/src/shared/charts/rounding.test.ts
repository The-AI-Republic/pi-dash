// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared bar end-rounding tests (parity/specs/analytics.md §10,
// NEWFRONT-732).
import { describe, expect, it } from "vitest";

import { BAR_END_RADIUS, singleBarRadius, stackedBarRadii, stackedSegmentRadius } from "./rounding.js";

describe("singleBarRadius", () => {
  it("rounds both ends of every bar", () => {
    expect(singleBarRadius()).toEqual([BAR_END_RADIUS, BAR_END_RADIUS, BAR_END_RADIUS, BAR_END_RADIUS]);
  });

  it("honors a custom radius", () => {
    expect(singleBarRadius(8)).toEqual([8, 8, 8, 8]);
  });
});

describe("stackedSegmentRadius", () => {
  it("rounds the bottom of the lowest nonzero segment", () => {
    expect(stackedSegmentRadius([0, 4, 2], 1)).toEqual([0, 0, 4, 4]);
  });

  it("rounds the top of the highest nonzero segment", () => {
    expect(stackedSegmentRadius([0, 4, 2], 2)).toEqual([4, 4, 0, 0]);
  });

  it("rounds both ends of a lone nonzero segment", () => {
    expect(stackedSegmentRadius([0, 4, 0], 1)).toEqual([4, 4, 4, 4]);
  });

  it("leaves interior and zero segments square", () => {
    expect(stackedSegmentRadius([1, 2, 3], 1)).toEqual([0, 0, 0, 0]);
    expect(stackedSegmentRadius([0, 0], 0)).toEqual([0, 0, 0, 0]);
  });

  it("honors a custom radius", () => {
    expect(stackedSegmentRadius([1, 2], 0, 6)).toEqual([0, 0, 6, 6]);
  });
});

describe("stackedBarRadii", () => {
  it("rounds only the extreme ends across stacked segments", () => {
    expect(stackedBarRadii([[2, 5, 3]])).toEqual([
      [
        [0, 0, 4, 4],
        [0, 0, 0, 0],
        [4, 4, 0, 0],
      ],
    ]);
  });

  it("skips zero-value segments when finding the extremes", () => {
    expect(stackedBarRadii([[0, 5, 0]])).toEqual([
      [
        [0, 0, 0, 0],
        [4, 4, 4, 4],
        [0, 0, 0, 0],
      ],
    ]);
  });

  it("leaves an all-zero bar fully square", () => {
    expect(stackedBarRadii([[0, 0]])).toEqual([
      [
        [0, 0, 0, 0],
        [0, 0, 0, 0],
      ],
    ]);
  });

  it("resolves each bar independently and honors a custom radius", () => {
    expect(
      stackedBarRadii(
        [
          [1, 2],
          [0, 0, 7],
        ],
        6
      )
    ).toEqual([
      [
        [0, 0, 6, 6],
        [6, 6, 0, 0],
      ],
      [
        [0, 0, 0, 0],
        [0, 0, 0, 0],
        [6, 6, 6, 6],
      ],
    ]);
  });

  it("returns no rows for no bars", () => {
    expect(stackedBarRadii([])).toEqual([]);
  });
});
