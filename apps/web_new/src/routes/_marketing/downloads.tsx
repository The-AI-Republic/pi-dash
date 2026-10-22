// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing downloads route (CLOUD-004 frame; the page body is owned
// by NEWFRONT-648, CLOUD-006–009). Mounted only when the edition declares
// /downloads; the lazy surface renders the intro page until it lands.
// The route contributes only its declaration to the router and nothing to
// the initial bundle.
import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/_marketing/downloads")({});
