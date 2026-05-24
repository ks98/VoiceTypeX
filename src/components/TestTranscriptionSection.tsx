// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import Button from "./Button";
import {
  ipcGetWhisperBackend,
  ipcRunTestTranscription,
  type TestTranscriptionResult,
  type WhisperBackendInfo,
} from "../lib/tauri";
import { useT } from "../i18n";

const TEST_DURATION = 5;

export default function TestTranscriptionSection(): JSX.Element {
  const t = useT();
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

  const btnLabel = running
    ? countdown !== null
      ? t("transcription_test.btn.recording", { seconds: countdown })
      : t("transcription_test.btn.transcribing")
    : t("transcription_test.btn.idle");

  return (
    <div className="flex flex-col gap-3 border border-outline rounded-md p-4 bg-surface">
      <div>
        <h2 className="text-lg font-semibold text-fg">
          {t("transcription_test.title")}
        </h2>
        <p className="text-xs text-fg-faint mt-1">
          {t("transcription_test.intro", { seconds: TEST_DURATION })}
        </p>
        {backend ? (
          <div className="mt-2 text-xs text-fg-muted">
            {t("transcription_test.backend_label")}{" "}
            <span className="text-status-done font-mono">
              {backend.backend}
            </span>{" "}
            {t("transcription_test.backend_speedup", {
              factor: backend.expected_speedup.toFixed(1),
            })}{" "}
            <span className="text-fg-faint">{backend.description}</span>
          </div>
        ) : null}
      </div>
      <div className="flex items-center gap-3">
        <Button onClick={() => void onRun()} disabled={running}>
          {btnLabel}
        </Button>
      </div>
      {error ? (
        <div className="text-xs text-status-error">{error}</div>
      ) : null}
      {result ? (
        <div className="flex flex-col gap-2 text-sm">
          <div className="flex gap-4">
            <div>
              <div className="text-xs text-fg-faint">
                {t("transcription_test.metric.rtf")}
              </div>
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
              <div className="text-xs text-fg-faint">
                {t("transcription_test.metric.processing_time")}
              </div>
              <div className="font-mono text-lg text-fg">
                {(result.processing_ms / 1000).toFixed(1)} s
              </div>
            </div>
            <div>
              <div className="text-xs text-fg-faint">
                {t("transcription_test.metric.audio")}
              </div>
              <div className="font-mono text-lg text-fg">
                {result.audio_seconds.toFixed(0)} s
              </div>
            </div>
          </div>
          <div>
            <div className="text-xs text-fg-faint">
              {t("transcription_test.metric.recognized")}
            </div>
            <div className="bg-elevated border border-outline rounded-md p-2 text-xs font-mono text-fg-muted mt-1">
              {result.text || t("transcription_test.empty_text")}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
