/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useState } from "react";
// hooks
import { useAppRouter } from "@/hooks/use-app-router";
// layouts
import { DevErrorComponent } from "./dev";
import { ProdErrorComponent } from "./prod";

// A stale-deploy failure: the user's cached index.html references a hashed
// chunk the new build no longer serves, so a lazy() import 404s. Vite/Rollup
// and each browser word this differently.
function isChunkLoadError(error: unknown): boolean {
  const err = error as { name?: string; message?: string } | null;
  const message = err?.message ?? "";
  return (
    err?.name === "ChunkLoadError" ||
    /failed to fetch dynamically imported module/i.test(message) ||
    /error loading dynamically imported module/i.test(message) ||
    /importing a module script failed/i.test(message)
  );
}

// Only reload once per short window so a genuinely-missing asset (build/CDN
// gap, not just stale) falls through to the error screen instead of looping.
const RELOAD_GUARD_KEY = "pi-dash:chunk-reload-at";
const RELOAD_GUARD_MS = 10_000;

const handleReload = () => window.location.reload();

export function CustomErrorComponent({ error }: { error: unknown }) {
  // router
  const router = useAppRouter();
  // Recover from stale-deploy chunk errors by reloading once. Start in the
  // recovering state so we don't flash the maintenance screen before reloading.
  const [recovering, setRecovering] = useState(() => isChunkLoadError(error));

  const handleGoHome = () => router.push("/");

  useEffect(() => {
    if (!isChunkLoadError(error)) return;
    const last = Number(sessionStorage.getItem(RELOAD_GUARD_KEY) ?? 0);
    if (Date.now() - last > RELOAD_GUARD_MS) {
      sessionStorage.setItem(RELOAD_GUARD_KEY, String(Date.now()));
      window.location.reload();
    } else {
      // Already tried recently — the asset is likely gone, show the real error.
      setRecovering(false);
    }
  }, [error]);

  if (import.meta.env.DEV) {
    return <DevErrorComponent error={error} onGoHome={handleGoHome} onReload={handleReload} />;
  }

  // About to reload; render nothing rather than the maintenance screen.
  if (recovering) return null;

  return <ProdErrorComponent onGoHome={handleGoHome} />;
}
