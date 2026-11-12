// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud free-cap paywall (CLOUD-010, from parity/specs/cloud.md §4). A
// creation call refused with the exact quota payload prompts once — a
// confirmation naming the limit — and accepting opens the payload's
// upgrade address (or the pricing page when it carries none) in a new
// tab. A guard flag keeps prompts from stacking when several calls fail
// together, and refusals without the exact shape never prompt. The prompt
// and the tab open arrive injected (the package never touches window);
// the cloud edition lists this in `api` and the OSS edition lists nothing.
import type { Middleware, TransportResponse } from "../client.js";
import { QuotaExceededPayload } from "../contracts/cloud-session.js";

/** Fallback destination when the refusal carries no upgrade address. */
export const PAYWALL_PRICING_PATH = "/pricing";

/** The injected prompt surface: a confirmation plus a new-tab open. */
export interface PaywallPrompt {
  /** Show the quota prompt; resolves true when the user accepts the upgrade. */
  confirm(message: string): boolean | Promise<boolean>;
  /** Open the upgrade destination in a new tab. */
  openUpgrade(url: string): void | Promise<void>;
}

/** The single prompt, naming the limit the payload carries. */
export function paywallMessage(limit: number, used: number): string {
  return `You've reached the free creation limit (${used} of ${limit} used). Upgrade for unlimited creation?`;
}

/** The refusal off a 402 body: the quota shape when it matches, else null. */
function quotaRefusal(body: unknown): QuotaExceededPayload | null {
  const parsed = QuotaExceededPayload.safeParse(body);
  return parsed.success ? parsed.data : null;
}

/** Absolute upgrade address: the payload's, else pricing on the request's origin. */
function upgradeTarget(payloadUrl: string | undefined, requestUrl: string): string {
  if (payloadUrl !== undefined) return payloadUrl;
  try {
    return new URL(PAYWALL_PRICING_PATH, requestUrl).toString();
  } catch {
    return PAYWALL_PRICING_PATH;
  }
}

export function createCloudPaywallMiddleware(prompt: PaywallPrompt): Middleware {
  let prompting = false;
  return async (request, next) => {
    const response = await next(request);
    if (response.status !== 402) return response;
    let text: string;
    let body: unknown;
    try {
      text = await response.text();
      body = text ? (JSON.parse(text) as unknown) : null;
    } catch {
      // An unreadable body cannot match the quota shape; leave it untouched.
      return response;
    }
    const refusal = quotaRefusal(body);
    // The body was consumed above; hand the caller an equivalent answer.
    const revived: TransportResponse = { ...response, text: async () => text };
    if (refusal === null || prompting) return revived;
    prompting = true;
    try {
      const accepted = await prompt.confirm(paywallMessage(refusal.limit, refusal.used));
      if (accepted) {
        try {
          await prompt.openUpgrade(upgradeTarget(refusal.upgrade_url, request.url));
        } catch {
          // A blocked popup must not mask the refusal the caller awaits.
        }
      }
    } finally {
      prompting = false;
    }
    return revived;
  };
}
