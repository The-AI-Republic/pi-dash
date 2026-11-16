// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration session identity (parity/specs/pages.md §8.2, §11.6,
// PAGE-066). The socket handshake carries the current user as JSON, and
// the cursor colour derives deterministically from the user id, so every
// client paints the same person the same colour without ever exchanging
// a palette.

/** The signed-in participant a session opens as. */
export interface PageLiveUser {
  id: string;
  name: string;
}

/** The handshake payload: identity plus the derived cursor colour. */
export interface PageLiveIdentity {
  id: string;
  name: string;
  color: string;
}

/** FNV-1a over the id's code units: stable across sessions and devices. */
function hashUserId(userId: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < userId.length; index += 1) {
    hash ^= userId.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** The id's hue on the colour ring, in degrees. */
export function liveHueOf(userId: string): number {
  return hashUserId(userId) % 360;
}

function hueChannel(position: number): number {
  const wrapped = position < 0 ? position + 1 : position > 1 ? position - 1 : position;
  if (wrapped < 1 / 6) return wrapped * 6;
  if (wrapped < 1 / 2) return 1;
  if (wrapped < 2 / 3) return (2 / 3 - wrapped) * 6;
  return 0;
}

/**
 * One participant's cursor colour: the id hashes to a hue on a vivid
 * ring, so the colour is stable for the person and distinct between
 * people. Always a lowercase `#rrggbb` triple.
 */
export function liveCursorColor(userId: string): string {
  const hue = liveHueOf(userId) / 360;
  const saturation = 0.62;
  const lightness = 0.45;
  const high = lightness * (1 + saturation);
  const low = 2 * lightness - high;
  const channels = [hue + 1 / 3, hue, hue - 1 / 3].map((position) =>
    Math.round((hueChannel(position) * (high - low) + low) * 255)
  );
  return `#${channels.map((channel) => channel.toString(16).padStart(2, "0")).join("")}`;
}

/** The identity a session handshakes with. */
export function liveIdentityOf(user: PageLiveUser): PageLiveIdentity {
  return { id: user.id, name: user.name, color: liveCursorColor(user.id) };
}

/** The handshake token: the identity as JSON (spec §11.6). */
export function liveIdentityToken(user: PageLiveUser): string {
  return JSON.stringify(liveIdentityOf(user));
}
