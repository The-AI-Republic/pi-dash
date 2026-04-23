/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type { IPod } from "@pi-dash/types";
import { APIService } from "../api.service";

/**
 * Pi Dash Pod CRUD client. Session-authenticated; mounted at
 * /api/runners/pods/ on the Django server. See
 * .ai_design/issue_runner/design.md §8.1.
 */
export class PodService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  async list(workspaceId: string): Promise<IPod[]> {
    return this.get("/api/runners/pods/", { params: { workspace: workspaceId } })
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async create(input: { workspace: string; name: string; description?: string }): Promise<IPod> {
    return this.post("/api/runners/pods/", input)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async get_one(podId: string): Promise<IPod> {
    return this.get(`/api/runners/pods/${podId}/`)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async update(podId: string, input: { name?: string; description?: string; is_default?: boolean }): Promise<IPod> {
    return this.patch(`/api/runners/pods/${podId}/`, input)
      .then((r) => r?.data)
      .catch((e) => {
        throw e?.response?.data;
      });
  }

  async remove(podId: string): Promise<void> {
    return this.delete(`/api/runners/pods/${podId}/`)
      .then(() => undefined)
      .catch((e) => {
        throw e?.response?.data;
      });
  }
}
