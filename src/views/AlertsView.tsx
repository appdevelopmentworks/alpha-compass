'use client';

import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  getAlerts,
  getAlertRules,
  isTauri,
  type Alert,
  type AlertRule,
} from '@/lib/ipc';
import { formatJst } from '@/lib/format';

function SeverityBadge({ severity }: { severity: string }) {
  const cls =
    severity === 'high'
      ? 'badge badge--err'
      : severity === 'medium'
        ? 'badge badge--idle'
        : 'badge badge--ok';
  const label = severity === 'high' ? '高' : severity === 'medium' ? '中' : severity;
  return <span className={cls}>{label}</span>;
}

export default function AlertsView() {
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [rules, setRules] = useState<AlertRule[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      return;
    }
    try {
      const [a, r] = await Promise.all([getAlerts(50), getAlertRules()]);
      setAlerts(a);
      setRules(r);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
    if (!isTauri()) return;
    // Live updates: prepend newly fired alerts.
    const un = listen<Alert>('alert:fired', (ev) => {
      setAlerts((prev) => [ev.payload, ...prev.filter((a) => a.id !== ev.payload.id)]);
    });
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          アラート
        </span>
        <button className="btn" onClick={load}>
          再読込
        </button>
      </div>

      {error && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{error}</p>
        </div>
      )}

      {/* Rules */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">ルール</span>
        </div>
        {rules.length > 0 ? (
          <ul className="feed">
            {rules.map((r) => (
              <li className="feed__item" key={r.id}>
                <div className="feed__head">
                  <span className="feed__title">{r.name}</span>
                  <span className={r.enabled ? 'badge badge--ok' : 'badge badge--idle'}>
                    {r.enabled ? '有効' : '無効'}
                  </span>
                </div>
                <div className="feed__meta">{r.condition}</div>
              </li>
            ))}
          </ul>
        ) : (
          <span className="badge badge--idle">ルールなし</span>
        )}
        <p className="note">
          アラートは複数の独立シグナルが一致したときのみ発火します（単一ソースでは鳴らさない）。閾値・最小一致数は設定で調整予定。
        </p>
      </section>

      {/* History */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">履歴</span>
          <span className="panel__freshness">{alerts.length} 件</span>
        </div>
        {alerts.length === 0 ? (
          <p className="note">
            まだアラートはありません。複数シグナルが同方向に一致すると発火します（現在の地合いは平常）。
          </p>
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>重要度</th>
                <th>内容</th>
                <th>トリガー</th>
                <th>時刻</th>
              </tr>
            </thead>
            <tbody>
              {alerts.map((a) => (
                <tr key={a.id}>
                  <td>
                    <SeverityBadge severity={a.severity} />
                  </td>
                  <td>{a.title}</td>
                  <td>{a.triggers.join('、')}</td>
                  <td>{formatJst(a.ts)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}
