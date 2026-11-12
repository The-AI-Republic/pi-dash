// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing home route (CLOUD-003). The lazy marketing surface renders the
// body for this address, so the route contributes only its declaration to
// the router and nothing to the initial bundle. It mounts only when the
// edition declares /home (the layout answers not-found otherwise).
import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/_marketing/home")({});
