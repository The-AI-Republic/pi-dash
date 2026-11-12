// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
export function defaultEditionModule(appDir: string): string;
export function resolveEditionModule(
  env: NodeJS.ProcessEnv,
  appDir: string
): { modulePath: string; fsAllow: string[] | null };
