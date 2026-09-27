// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

import { API_BASE_URL } from "@pi-dash/constants";
import type { IInstanceRunnersPage } from "@pi-dash/types";
import { APIService } from "../api.service";

// Instance-admin client for the cross-workspace runner list. Behind
// InstanceAdminPermission server-side.
export class InstanceRunnerService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async list(
    params: { page?: number; provisioning?: string; status?: string; workspace?: string } = {}
  ): Promise<IInstanceRunnersPage> {
    return this.get("/api/instances/runners/", { params })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
