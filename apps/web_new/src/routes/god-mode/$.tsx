// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// God-mode catch-all (ADM-011, from parity/specs/admin.md §1.11).
// Unknown console addresses render the not-found screen with or without
// a session: this route sits outside the guarded home and dashboard
// trees, so no guard preempts it.
import { createFileRoute } from "@tanstack/react-router";

import { AdminNotFoundScreen } from "../../features/admin/components/AdminNotFoundScreen.js";

export const Route = createFileRoute("/god-mode/$")({
  component: AdminNotFoundScreen,
});
