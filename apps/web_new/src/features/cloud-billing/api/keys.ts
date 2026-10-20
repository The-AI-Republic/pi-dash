// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Plan snapshot keys (CLOUD-038). Account-scoped, not workspace-scoped:
// the plan belongs to the signed-in account and is the same in every
// workspace the badge appears in.

export const cloudBillingKeys = {
  plan: ["cloud-billing", "plan"] as const,
};
