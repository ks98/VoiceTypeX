// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";

export default function App(): JSX.Element {
  const [active] = useState<string>("Phase 1 — Scaffolding");

  return (
    <main className="min-h-screen flex flex-col items-center justify-center p-8 gap-4">
      <h1 className="text-3xl font-bold text-brand-500">VoiceTypeX</h1>
      <p className="text-slate-400">{active}</p>
      <p className="text-xs text-slate-600 mt-8">
        Diktiere — VoiceTypeX schreibt es. Lokal, Cloud, oder beides. Du
        entscheidest pro Modus.
      </p>
    </main>
  );
}
