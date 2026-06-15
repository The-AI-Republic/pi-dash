/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
// hooks
import { useAppRouter } from "@/hooks/use-app-router";
// utils
import { isChunkLoadError, reloadOnceForStaleChunk } from "./chunk-reload";
// layouts
import { DevErrorComponent } from "./dev";
import { ProdErrorComponent } from "./prod";

const handleReload = () => window.location.reload();

export function CustomErrorComponent({ error }: { error: unknown }) {
  // router
  const router = useAppRouter();
  // Recover from stale-deploy chunk errors by reloading once. Start in the
  // recovering state so we don't flash the maintenance screen before reloading.
  // (The vite:preloadError listener in entry.client usually catches these first;
  // this boundary is the fallback for chunk errors that slip past it.)
  const [recovering, setRecovering] = useState(() => isChunkLoadError(error));

  const handleGoHome = () => router.push("/");

  useEffect(() => {
    if (!isChunkLoadError(error)) return;
    // If we reloaded too recently, the asset is likely gone — show the real error.
    if (!reloadOnceForStaleChunk()) setRecovering(false);
  }, [error]);

  if (import.meta.env.DEV) {
    return <DevErrorComponent error={error} onGoHome={handleGoHome} onReload={handleReload} />;
  }

  // About to reload; render nothing rather than the maintenance screen.
  if (recovering) return null;

  return <ProdErrorComponent onGoHome={handleGoHome} />;
}
