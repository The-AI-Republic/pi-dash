/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { Request, Response } from "express";
import { Controller, Get } from "@apple-pi-dash/decorators";
import { env } from "@/env";

@Controller("/health")
export class HealthController {
  @Get("/")
  async healthCheck(_req: Request, res: Response) {
    res.status(200).json({
      status: "OK",
      timestamp: new Date().toISOString(),
      version: env.APP_VERSION,
    });
  }
}
