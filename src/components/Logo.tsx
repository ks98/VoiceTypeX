// SPDX-License-Identifier: GPL-3.0-or-later

interface LogoProps {
  className?: string;
}

/*
 * VoiceTypeX mark "Wave-to-Caret":
 * An audio waveform (4 bars) flows into a text-insertion cursor.
 * `fill="currentColor"` makes it theme-aware — e.g. text-brand in
 * the sidebar header, text-fg in the onboarding hero.
 *
 * Source-of-truth identical to src-tauri/icons/source/logo.svg
 * (on edits, keep both in sync — the SVG file is rendered into the
 * PNG bundle icons via rsvg-convert).
 */
export default function Logo({ className = "" }: LogoProps): JSX.Element {
  return (
    <svg
      viewBox="0 0 64 64"
      xmlns="http://www.w3.org/2000/svg"
      fill="currentColor"
      className={className}
      aria-hidden
    >
      <rect x="8" y="22" width="5" height="20" rx="2.5" />
      <rect x="17" y="14" width="5" height="36" rx="2.5" />
      <rect x="26" y="18" width="5" height="28" rx="2.5" />
      <rect x="35" y="24" width="5" height="16" rx="2.5" />
      <rect x="46" y="14" width="10" height="2.5" rx="1.25" />
      <rect x="49.25" y="14" width="3.5" height="36" rx="1.75" />
      <rect x="46" y="47.5" width="10" height="2.5" rx="1.25" />
    </svg>
  );
}
