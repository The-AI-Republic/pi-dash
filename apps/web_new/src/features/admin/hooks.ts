// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin data hooks (Data layer page). Components read the instance
// bootstrap and the native-POST forgery token through these, never
// through raw client calls.

import { useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../core/api/client.js";
import { adminCsrfQueryOptions, instanceBootstrapQueryOptions } from "./queries.js";

/**
 * The single instance-info read the entry gate branches on. Pending
 * while unresolved, errored when the fetch fails, settled after —
 * never refetched behind the screens' backs.
 */
export function useInstanceBootstrap() {
  return useQuery(instanceBootstrapQueryOptions(getAppClient()));
}

/** The forgery token the native setup/sign-in POSTs carry. */
export function useAdminCsrfToken() {
  return useQuery(adminCsrfQueryOptions(getAppClient()));
}
