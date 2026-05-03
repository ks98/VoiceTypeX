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
    <div className="flex flex-col gap-3 border border-slate-800 rounded p-4">
      <div>
        <h2 className="text-lg font-semibold text-slate-200">
          Test-Transkription
        </h2>
        <p className="text-xs text-slate-500 mt-1">
          Nimmt {TEST_DURATION} Sekunden Audio vom Default-Mikrofon auf,
          transkribiert lokal mit dem konfigurierten Whisper-Modell, und meldet
          den Real-Time-Factor (RTF). RTF &lt; 1 bedeutet schneller als
          Echtzeit.
        </p>
        {backend ? (
          <div className="mt-2 text-xs text-slate-400">
            Aktives Backend:{" "}
            <span className="text-emerald-400 font-mono">
              {backend.backend}
            </span>{" "}
            (~{backend.expected_speedup.toFixed(1)}× ggü. CPU-Default).{" "}
            <span className="text-slate-500">{backend.description}</span>
          </div>
        ) : null}
      </div>
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => void onRun()}
          disabled={running}
          className="px-3 py-2 rounded bg-brand-700 hover:bg-brand-500 disabled:bg-slate-800 disabled:text-slate-500 text-sm"
        >
          {running
            ? countdown !== null
              ? `Aufnahme… ${countdown}`
              : "Transkribiere…"
            : "Test starten (5 s)"}
        </button>
      </div>
      {error ? <div className="text-xs text-red-400">{error}</div> : null}
      {result ? (
        <div className="flex flex-col gap-2 text-sm">
          <div className="flex gap-4">
            <div>
              <div className="text-xs text-slate-500">RTF</div>
              <div
                className={
                  result.rtf < 1
                    ? "text-emerald-400 font-mono text-lg"
                    : "text-amber-400 font-mono text-lg"
                }
              >
                {result.rtf.toFixed(2)}
              </div>
            </div>
            <div>
              <div className="text-xs text-slate-500">Verarbeitungszeit</div>
              <div className="font-mono text-lg text-slate-300">
                {(result.processing_ms / 1000).toFixed(1)} s
              </div>
            </div>
            <div>
              <div className="text-xs text-slate-500">Audio</div>
              <div className="font-mono text-lg text-slate-300">
                {result.audio_seconds.toFixed(0)} s
              </div>
            </div>
          </div>
          <div>
            <div className="text-xs text-slate-500">Erkannter Text</div>
            <div className="bg-slate-900 border border-slate-800 rounded p-2 text-xs font-mono text-slate-300 mt-1">
              {result.text || "(leer)"}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
