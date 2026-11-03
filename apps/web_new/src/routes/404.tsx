// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fixed generic not-found address (SPC-002, SPC-009). Legacy links that do
// not resolve land here rather than on an anchor-specific illustration, so
// the visitor sees the shared guidance page and no board chrome.

import { createFileRoute } from "@tanstack/react-router";

import { NotFoundPage } from "../shared/shell/NotFoundPage.js";

export const Route = createFileRoute("/404")({
  component: NotFoundPage,
});
