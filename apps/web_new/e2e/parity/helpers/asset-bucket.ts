// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Object-storage bridge for runs against apps/web_new (NEWFRONT-550). In
// production the app and the bucket share one origin: Caddy routes the
// bucket path to object storage, and the API signs uploads and fills
// redirects for that same host. The parity stack keeps that topology for
// the oracle, but web_new runs from its own dev server, which routes only
// `/api` and `/auth`. Presigned uploads and the images they produce are
// therefore addressed at the dev origin, where nothing serves them.
//
// This bridge restores the one-origin shape for the browser: a request for
// the bucket path on the dev origin is continued against the parity proxy,
// which does route it to object storage. The `host` header keeps the dev
// origin, because the signature covers the host it was signed for; the
// method, headers and multipart body pass through untouched.
import type { Page } from "@playwright/test";

import { parityOracleUrl } from "./parity-env.js";

/** The bucket the parity stack creates and signs uploads for. */
export const PARITY_ASSET_BUCKET = "parity19-assets";

/** The bucket path, with or without a trailing slash or query. */
export const PARITY_ASSET_BUCKET_PATTERN = new RegExp(`/${PARITY_ASSET_BUCKET}(/|\\?|$)`);

/** Origin of the parity proxy that serves the bucket. */
export function parityAssetProxyOrigin(env: Readonly<Record<string, string | undefined>> = process.env): string {
  return new URL(parityOracleUrl(env)).origin;
}

/**
 * Route the bucket path from the page's origin to the parity proxy. Safe to
 * call more than once: the caller records whether it already installed the
 * bridge for this page.
 */
export async function bridgeParityAssetBucket(
  page: Page,
  proxyOrigin: string = parityAssetProxyOrigin()
): Promise<void> {
  await page.route(PARITY_ASSET_BUCKET_PATTERN, async (route) => {
    const request = route.request();
    const target = new URL(request.url());
    await route
      .continue({
        url: `${proxyOrigin}${target.pathname}${target.search}`,
        headers: { ...request.headers(), host: target.host },
      })
      .catch(() => undefined);
  });
}
