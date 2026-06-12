// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

import { API_BASE_URL } from "@pi-dash/constants";
import type { ILoopJob, ILoopJobInput, ILoopTargetsPage } from "@pi-dash/types";
import { APIService } from "../api.service";

// Instance-admin client for managing loop ("Auto Project Management") jobs and
// observing their per-edge targets. Behind InstanceAdminPermission server-side.
export class InstanceLoopService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async list(): Promise<ILoopJob[]> {
    return this.get("/api/instances/loop/jobs/")
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async retrieve(jobId: string): Promise<ILoopJob> {
    return this.get(`/api/instances/loop/jobs/${jobId}/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async create(data: ILoopJobInput): Promise<ILoopJob> {
    return this.post("/api/instances/loop/jobs/", data)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async update(jobId: string, data: ILoopJobInput): Promise<ILoopJob> {
    return this.patch(`/api/instances/loop/jobs/${jobId}/`, data)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async destroy(jobId: string): Promise<void> {
    return this.delete(`/api/instances/loop/jobs/${jobId}/`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async listTargets(
    jobId: string,
    params: { page?: number; skip_reason?: string; status?: string } = {}
  ): Promise<ILoopTargetsPage> {
    return this.get(`/api/instances/loop/jobs/${jobId}/targets/`, { params })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
