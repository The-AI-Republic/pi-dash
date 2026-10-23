// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Generic unmatched public path (SPC-009). This is the shared guidance
// page, deliberately not the anchor or board illustration, and it never
// renders board chrome.

import { createFileRoute } from "@tanstack/react-router";

import { NotFoundPage } from "../../shared/shell/NotFoundPage.js";

export const Route = createFileRoute("/spaces/$")({
  component: NotFoundPage,
});
