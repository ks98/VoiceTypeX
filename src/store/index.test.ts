// SPDX-License-Identifier: GPL-3.0-or-later
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type Mock,
} from "vitest";
import type { Settings } from "../lib/types";
import type { ipcSetSettings } from "../lib/tauri";

// Stub the IPC layer so update() never touches a real Tauri backend.
// ipcSetSettings is the yield point update() serializes around (#53),
// so the mock is deferrable: each call parks until we resolve it, which
// lets us interleave two overlapping update() calls deterministically.
vi.mock("../lib/tauri", () => ({
  ipcSetSettings: vi.fn(),
  // update() doesn't call these, but the store module imports them at
  // load time — stub them so the import resolves.
  ipcGetModes: vi.fn(),
  ipcGetSettings: vi.fn(),
  ipcListAudioDevices: vi.fn(),
  retryWhileUnmanaged: vi.fn(),
}));

// The store carries a module-level serialization queue (updateQueue).
// Re-import both the store and the IPC mock fresh per test
// (vi.resetModules below) so (a) a bailed test can never wedge the next
// one's queue, and (b) the mock instance the test drives is the exact
// one the freshly-imported store closes over.
type SettingsStore = typeof import("./index").useSettingsStore;
let useSettingsStore: SettingsStore;
let setSettingsMock: Mock<typeof ipcSetSettings>;

// A minimal but complete Settings, so { ...current, ...partial } merges
// against a real shape (snake_case, mirroring the Rust type).
function baseSettings(): Settings {
  return {
    audio_input_device: null,
    whisper_model_path: null,
    whisper_default_slot: "large-v3-turbo",
    autostart: false,
    ollama_url: "http://localhost:11434",
    ollama_keep_alive: "5m",
    llm_default_slot: "gemma",
    llm_model_path: null,
    onboarding_done: false,
    whisper_n_threads: null,
    whisper_beam_size: 2,
    menu_hotkey: "F9",
    last_selected_mode_id: null,
    locale: "en",
  };
}

/** A promise plus its resolve/reject handles, for deferred IPC. */
function deferred(): {
  promise: Promise<void>;
  resolve: () => void;
  reject: (e: unknown) => void;
} {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(async () => {
  // Fresh module instances → fresh updateQueue, no cross-test leakage.
  vi.resetModules();
  const tauri = await import("../lib/tauri");
  setSettingsMock = vi.mocked(tauri.ipcSetSettings);
  setSettingsMock.mockReset();
  ({ useSettingsStore } = await import("./index"));
  useSettingsStore.setState({
    settings: baseSettings(),
    audioDevices: [],
    loading: false,
    error: null,
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("useSettingsStore.update — merge correctness", () => {
  it("merges a partial onto current settings and persists the merged result", async () => {
    setSettingsMock.mockResolvedValue(undefined);

    await useSettingsStore.getState().update({ whisper_beam_size: 5 });

    // Persisted the merged object, not just the partial.
    expect(setSettingsMock).toHaveBeenCalledTimes(1);
    const persisted = setSettingsMock.mock.calls[0]?.[0];
    expect(persisted).toMatchObject({
      whisper_beam_size: 5,
      menu_hotkey: "F9", // untouched base field carried through
      locale: "en",
    });

    // Store reflects the merge.
    expect(useSettingsStore.getState().settings?.whisper_beam_size).toBe(5);
    expect(useSettingsStore.getState().settings?.menu_hotkey).toBe("F9");
    expect(useSettingsStore.getState().error).toBeNull();
  });

  it("is a no-op when there are no settings yet (load() hasn't run)", async () => {
    useSettingsStore.setState({ settings: null });
    setSettingsMock.mockResolvedValue(undefined);

    await useSettingsStore.getState().update({ whisper_beam_size: 5 });

    expect(setSettingsMock).not.toHaveBeenCalled();
    expect(useSettingsStore.getState().settings).toBeNull();
  });
});

describe("useSettingsStore.update — no lost update under overlap (#53)", () => {
  it("the second overlapping update merges onto the first's result; no field dropped", async () => {
    // First persist parks until we release it; the second update() must
    // queue behind it and only read the (already-merged) settings after
    // the first write lands.
    const first = deferred();
    const second = deferred();
    setSettingsMock
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    // Two overlapping calls, different fields, fired back-to-back without
    // awaiting the first.
    const p1 = useSettingsStore.getState().update({ menu_hotkey: "F10" });
    const p2 = useSettingsStore.getState().update({ whisper_beam_size: 7 });

    // Let the first run() start and reach its parked ipcSetSettings.
    await vi.waitFor(() => expect(setSettingsMock).toHaveBeenCalledTimes(1));

    // Serialization proof: while the first IPC is still pending, the
    // second update()'s body has NOT run — only one persist so far, and
    // it carries the FIRST field merged onto the original base. A naive
    // non-serialized update() would have read the same base and called
    // ipcSetSettings twice by now (last-writer-wins → drop).
    expect(setSettingsMock).toHaveBeenCalledTimes(1);
    expect(setSettingsMock.mock.calls[0]?.[0]).toMatchObject({
      menu_hotkey: "F10",
      whisper_beam_size: 2, // base value — the 2nd field isn't in yet
    });

    // Release the first write. Its set({settings}) lands, then the second
    // update() reads the merged settings as its base and persists.
    first.resolve();
    await vi.waitFor(() => expect(setSettingsMock).toHaveBeenCalledTimes(2));

    const secondPayload = setSettingsMock.mock.calls[1]?.[0];
    // The smoking gun: the second persist carries BOTH fields. A naive
    // implementation would persist { ...staleBase, whisper_beam_size: 7 }
    // and silently drop menu_hotkey: "F10".
    expect(secondPayload).toMatchObject({
      menu_hotkey: "F10",
      whisper_beam_size: 7,
    });

    second.resolve();
    await Promise.all([p1, p2]);

    // Final store state has both fields — neither update was lost.
    const finalSettings = useSettingsStore.getState().settings;
    expect(finalSettings?.menu_hotkey).toBe("F10");
    expect(finalSettings?.whisper_beam_size).toBe(7);
    expect(useSettingsStore.getState().error).toBeNull();
  });
});

describe("useSettingsStore.update — persist failure", () => {
  it("surfaces error and leaves settings intact when ipcSetSettings rejects", async () => {
    setSettingsMock.mockRejectedValue(new Error("disk full"));
    const before = useSettingsStore.getState().settings;

    await useSettingsStore.getState().update({ whisper_beam_size: 9 });

    expect(useSettingsStore.getState().error).toContain("disk full");
    // settings unchanged — the failed write must not commit to the store.
    expect(useSettingsStore.getState().settings).toEqual(before);
    expect(useSettingsStore.getState().settings?.whisper_beam_size).toBe(2);
  });

  it("a failed update does not wedge the queue: a later update still lands", async () => {
    setSettingsMock
      .mockRejectedValueOnce(new Error("transient"))
      .mockResolvedValueOnce(undefined);

    await useSettingsStore.getState().update({ whisper_beam_size: 3 });
    expect(useSettingsStore.getState().error).toContain("transient");

    await useSettingsStore.getState().update({ menu_hotkey: "F11" });
    expect(useSettingsStore.getState().settings?.menu_hotkey).toBe("F11");
  });
});
