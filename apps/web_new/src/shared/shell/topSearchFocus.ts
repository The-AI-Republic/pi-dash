// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Top-bar search focus requests (shell spec, SHELL-089). The palette's
// focus entry and its chord bump the counter; the search box focuses its
// input on every change. A counter rather than a flag so repeat
// activations refocus even while the box already holds focus.

import { create } from "zustand";

import { registerStoreReset } from "../../core/session/store.js";

interface TopSearchFocusState {
  request: number;
  requestTopSearchFocus: () => void;
}

const initial = { request: 0 };

export const useTopSearchFocus = create<TopSearchFocusState>()((set) => ({
  ...initial,
  requestTopSearchFocus: () => set((state) => ({ request: state.request + 1 })),
}));

registerStoreReset(() => {
  useTopSearchFocus.setState(initial);
});
