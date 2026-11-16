// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The size gate's budgets and its size-limit entries, kept apart from
// .size-limit.mjs so a unit test can pin them without a build on disk (the
// config resolves the build manifest at import time). Each number blocks: a
// reading above it fails the PR. Initial JS has this one ceiling and nothing
// else — no second, non-blocking number and no environment switch (owner
// decision 5 of 2026-11-10, NEWFRONT-1061), so the gate catches only an
// accidental large eager import (NEWFRONT-248) and ordinary slice growth is
// left to review. Initial JS covers the shell plus the first route, resolved
// by checks/size-initial.mjs; initial CSS covers every emitted stylesheet.
export const INITIAL_JS_CEILING = "276 KB";
export const INITIAL_CSS_CEILING = "30 KB";
export const INITIAL_CSS_GLOB = "dist/web/assets/*.css";

// The size-limit config array: one entry per measured budget, both gzipped.
export function sizeLimits(initialJs) {
  return [
    {
      name: "web initial JS (shell + first route)",
      path: initialJs,
      limit: INITIAL_JS_CEILING,
      gzip: true,
    },
    {
      name: "web initial CSS",
      path: INITIAL_CSS_GLOB,
      limit: INITIAL_CSS_CEILING,
      gzip: true,
    },
  ];
}
