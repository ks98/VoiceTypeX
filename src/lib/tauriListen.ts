// SPDX-License-Identifier: GPL-3.0-or-later
//
// Cancel-safe cleanup for Tauri event subscriptions inside React effects.
//
// `listen()` resolves asynchronously. The naive
// `void listen(...).then((fn) => unlistens.push(fn))` pattern leaks the
// listener when the component unmounts before the promise resolves: the
// effect's cleanup runs first against an empty array, then the handle is
// pushed afterwards and never unlistened. This is deterministic under
// React.StrictMode, which mounts → unmounts → remounts on the first commit.
//
// `listenAll` tracks a cancelled flag. The returned cleanup sets it and
// unlistens everything already resolved; any subscription that resolves
// after cancellation immediately unlistens itself.

import type { UnlistenFn } from "@tauri-apps/api/event";

/**
 * Wires up a set of `listen(...)` promises with cancel-safe cleanup.
 *
 * Pass the (unresolved) promises returned by `listen(...)`; the returned
 * function is the effect cleanup — call it on unmount. After cleanup any
 * still-pending subscription unlistens itself the moment it resolves, so
 * no listener can outlive the component.
 */
export function listenAll(subscriptions: Promise<UnlistenFn>[]): () => void {
  let cancelled = false;
  const unlistens: UnlistenFn[] = [];

  for (const sub of subscriptions) {
    void sub.then((unlisten) => {
      if (cancelled) {
        unlisten();
      } else {
        unlistens.push(unlisten);
      }
    });
  }

  return () => {
    cancelled = true;
    for (const unlisten of unlistens) unlisten();
  };
}
