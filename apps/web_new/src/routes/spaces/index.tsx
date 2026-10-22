// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Landing composition point (SPC-005, slice B). Slice A owns the route and
// keeps the public subtree shape; the landing auth card and signed-in
// guidance mount here in slice B rather than being invented early.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

export const Route = createFileRoute("/spaces/")({
  component: function SpacesLanding() {
    return <main data-testid="space-landing" className="min-h-screen bg-(--bg)" />;
  },
});
