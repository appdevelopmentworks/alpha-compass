'use client';

import { formatJst } from '@/lib/format';
import type { SourceMeta } from '@/lib/ipc';

const STATUS_LABEL: Record<string, string> = {
  ok: '最新',
  error: 'エラー',
  unavailable: '未取得',
  never: '未取得',
};

export function statusBadgeClass(status: string): string {
  if (status === 'ok') return 'badge badge--ok';
  if (status === 'error') return 'badge badge--err';
  return 'badge badge--idle';
}

/** Row of per-source freshness chips (source, status, JST last-fetched time). */
export function FreshnessChips({ metas }: { metas: SourceMeta[] }) {
  return (
    <div className="chips">
      {metas.map((m) => (
        <div className="chip" key={m.source} title={m.detail ?? ''}>
          <span className="chip__src">{m.source}</span>
          <span className={statusBadgeClass(m.status)}>
            {STATUS_LABEL[m.status] ?? m.status}
          </span>
          <span className="chip__time">
            {m.last_fetched_at ? formatJst(m.last_fetched_at) : '—'}
          </span>
        </div>
      ))}
    </div>
  );
}
