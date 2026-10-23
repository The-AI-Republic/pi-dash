// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Space boot gate (SPC-016 stage 1, SPC-018). The app-wide instance gate is
// bypassed for /spaces/* so the public subtree can make exactly one probe
// and render its own failure screen; it still honours the settled
// maintenance and unfinished-setup branches through the shared decision.

import { Spinner } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { decideAppInstanceGate } from "../../../core/instance/status.js";
import { spaceRuntime } from "../api/client.js";
import { spaceInstanceQueryOptions } from "../api/queries.js";
import { SpaceInstanceFailureScreen } from "./InstanceFailureScreen.js";

export function SpaceBootGate({ children }: { children: React.ReactNode }): React.ReactElement {
  const instance = useQuery(spaceInstanceQueryOptions(spaceRuntime.client));
  const branch = decideAppInstanceGate({
    isPending: instance.isPending,
    isError: instance.isError,
    data: instance.data,
  });
  if (branch === "loading") {
    return (
      <div
        data-testid="space-boot-loader"
        role="status"
        aria-label="Loading instance"
        className="flex min-h-screen items-center justify-center bg-(--bg)"
      >
        <Spinner size="large" label="Loading instance" />
      </div>
    );
  }
  if (branch === "maintenance") return <SpaceInstanceFailureScreen />;
  if (branch === "not-ready") return <SpaceInstanceFailureScreen />;
  return <>{children}</>;
}
