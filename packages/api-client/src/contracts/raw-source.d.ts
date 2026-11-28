// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Vite's `?raw` suffix imports a file's text. Tests use it to read the
// backend serializers they guard instead of keeping copies of them.
declare module "*?raw" {
  const source: string;
  export default source;
}
