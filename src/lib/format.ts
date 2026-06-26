/**
 * Japanese-first display formatting helpers.
 *
 * This is the seed of the formatting layer described in the architecture doc
 * (§3.3): JST time, Japanese number notation (円・万・億・%), market-hours
 * awareness. Session 0 only needs JST timestamp formatting for freshness
 * labels; the rest is filled in by later sessions.
 */

const JST_FORMATTER = new Intl.DateTimeFormat('ja-JP', {
  timeZone: 'Asia/Tokyo',
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hour12: false,
});

/** Format an ISO-8601 (UTC) timestamp as a JST wall-clock string. */
export function formatJst(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '—';
  return `${JST_FORMATTER.format(d)} JST`;
}

/** Format a number with fixed decimals; "—" for null/NaN. */
export function fmtNum(n: number | null | undefined, digits = 2): string {
  if (n === null || n === undefined || Number.isNaN(n)) return '—';
  return n.toLocaleString('ja-JP', {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
}

/** Format an integer with Japanese thousands grouping; "—" for null. */
export function fmtInt(n: number | null | undefined): string {
  if (n === null || n === undefined || Number.isNaN(n)) return '—';
  return Math.round(n).toLocaleString('ja-JP');
}

/** Format an already-percent number (e.g. 1.86 -> "+1.86%"). */
export function fmtPct(n: number | null | undefined, digits = 2): string {
  if (n === null || n === undefined || Number.isNaN(n)) return '—';
  const s = n.toFixed(digits);
  return `${n > 0 ? '+' : ''}${s}%`;
}

/** Format a fraction as a percent (e.g. 0.0186 -> "+1.86%"). */
export function fmtPctFrac(f: number | null | undefined, digits = 2): string {
  if (f === null || f === undefined || Number.isNaN(f)) return '—';
  return fmtPct(f * 100, digits);
}

/** CSS class name based on sign, for up/down coloring. */
export function signClass(n: number | null | undefined): string {
  if (n === null || n === undefined || Number.isNaN(n) || n === 0) return 'flat';
  return n > 0 ? 'up' : 'down';
}

/** Relative "X 秒前 / X 分前" label for freshness display. */
export function freshnessLabel(iso: string, now: Date = new Date()): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '取得時刻不明';
  const sec = Math.max(0, Math.round((now.getTime() - d.getTime()) / 1000));
  if (sec < 60) return `${sec} 秒前`;
  const min = Math.round(sec / 60);
  if (min < 60) return `${min} 分前`;
  const hr = Math.round(min / 60);
  return `${hr} 時間前`;
}
