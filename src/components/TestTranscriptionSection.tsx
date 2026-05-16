// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import {
  ipcGetWhisperBackend,
  ipcRunTestTranscription,
  type TestTranscriptionResult,
  type WhisperBackendInfo,
} from "../lib/tauri";

const TEST_DURATION = 5;

export default function TestTranscriptionSection(): JSX.Element {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<TestTranscriptionResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [backend, setBackend] = useState<WhisperBackendInfo | null>(null);

  useEffect(() => {
    void ipcGetWhisperBackend()
      .then(setBackend)
      .catch(() => null);
  }, []);

  const onRun = async () => {
    setRunning(true);
    setResult(null);
    setError(null);
    setCountdown(TEST_DURATION);

    const tick = window.setInterval(() => {
      setCountdown((prev) => (prev !== null && prev > 1 ? prev - 1 : null));
    }, 1000);

    try {
      const r = await ipcRunTestTranscription(TEST_DURATION);
      setResult(r);
    } catch (e) {
      setError(String(e));
    } finally {
      window.clearInterval(tick);
      setCountdown(null);
      setRunning(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 border border-outline rounded-md p-4 bg-surface">
      <div>
        <h2 className="text-lg font-semibold text-fg">Test-Transkription</h2>
        <p className="text-xs text-fg-faint mt-1">
          Nimmt {TEST_DURATION} Sekunden Audio vom Default-Mikrofon auf,
          transkribiert lokal mit dem konfigurierten Whisper-Modell, und meldet
          den Real-Time-Factor (RTF). RTF &lt; 1 bedeutet schneller als
          Echtzeit.
        </p>
        {backend ? (
          <div className="mt-2 text-xs text-fg-muted">
            Aktives Backend:{" "}
            <span className="text-status-done font-mono">
              {backend.backend}
            </span>{" "}
            (~{backend.expected_speedup.toFixed(1)}× ggü. CPU-Default).{" "}
            <span className="text-fg-faint">{backend.description}</span>
          </div>
        ) : null}
      </div>
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => void onRun()}
          disabled={running}
          className="inline-flex items-center px-3 py-2 rounded-md bg-brand text-brand-contrast text-sm font-medium hover:bg-brand-hover transition-colors disabled:bg-elevated disabled:text-fg-faint disabled:cursor-not-allowed"
        >
          {running
            ? countdown !== null
              ? `Aufnahme… ${countdown}`
              : "Transkribiere…"
            : "Test starten (5 s)"}
        </button>
      </div>
      {error ? (
        <div className="text-xs text-status-error">{error}</div>
      ) : null}
      {result ? (
        <div className="flex flex-col gap-2 text-sm">
          <div className="flex gap-4">
            <div>
              <div className="text-xs text-fg-faint">RTF</div>
              <div
                className={
                  result.rtf < 1
                    ? "text-status-done font-mono text-lg"
                    : "text-status-processing font-mono text-lg"
                }
              >
                {result.rtf.toFixed(2)}
              </div>
            </div>
            <div>
              <div className="text-xs text-fg-faint">Verarbeitungszeit</div>
              <div className="font-mono text-lg text-fg">
                {(result.processing_ms / 1000).toFixed(1)} s
              </div>
            </div>
            <div>
              <div className="text-xs text-fg-faint">Audio</div>
              <div className="font-mono text-lg text-fg">
                {result.audio_seconds.toFixed(0)} s
              </div>
            </div>
          </div>
          <div>
            <div className="text-xs text-fg-faint">Erkannter Text</div>
            <div className="bg-elevated border border-outline rounded-md p-2 text-xs font-mono text-fg-muted mt-1">
              {result.text || "(leer)"}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
