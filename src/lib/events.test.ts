// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { EVENTS } from "./events";

// Contract test — EVENT-NAME parity (#49, pairs with #48).
//
// This pins every `EVENTS` constant to its exact wire string. The same
// canonical list is hard-coded in the Rust unit test in
// `src-tauri/src/core/events.rs` for the subset the backend emits. Both
// sides are anchored to these literals, so a one-sided rename (a typo in
// `events.ts` OR a changed `pub const` in `events.rs`) breaks its own
// test instead of silently desyncing the emit/listen channel at runtime.
//
// Honest limit (contract-tests-over-codegen, no specta/ts-rs): the two
// sides do NOT auto-derive from each other — a coordinated rename of the
// constant AND this literal AND the Rust literal would pass. That is the
// accepted trade-off; the test catches the realistic failure (one side
// changed, the other forgotten).

const CANONICAL_EVENT_WIRE_NAMES = {
  STATE: "app://state",
  PARTIAL_TRANSCRIPT: "app://partial-transcript",
  ACTIVE_ENGINE: "app://active-engine",
  FOCUS_LOGS: "app://focus-logs",
  MODEL_DOWNLOAD_PROGRESS: "model-download-progress",
  LLM_MODEL_DOWNLOAD_PROGRESS: "llm-model-download-progress",
  LOCALE_CHANGED: "i18n://locale-changed",
} as const;

describe("EVENTS wire-name parity", () => {
  it("pins every constant to its exact wire string", () => {
    expect(EVENTS).toStrictEqual(CANONICAL_EVENT_WIRE_NAMES);
  });

  it("has exactly the expected set of keys (no add/remove drift)", () => {
    expect(Object.keys(EVENTS).sort()).toStrictEqual(
      Object.keys(CANONICAL_EVENT_WIRE_NAMES).sort(),
    );
  });
});
