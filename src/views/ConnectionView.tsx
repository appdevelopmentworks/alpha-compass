'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  pingSidecar,
  getDbStatus,
  isTauri,
  type PingResult,
  type DbStatus,
} from '@/lib/ipc';
import { formatJst } from '@/lib/format';

/** Session 0 connectivity check: frontend → Rust → sidecar → frontend. */
export default function ConnectionView() {
  const [ping, setPing] = useState<PingResult | null>(null);
  const [pinging, setPinging] = useState(false);
  const [db, setDb] = useState<DbStatus | null>(null);
  const [envError, setEnvError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      setEnvError(
        'Tauri webview の外で表示しています。IPC を使うには `npm run tauri dev` で起動してください。',
      );
      return;
    }
    getDbStatus()
      .then(setDb)
      .catch((e) => setEnvError(String(e)));
  }, []);

  const onPing = useCallback(async () => {
    if (!isTauri()) return;
    setPinging(true);
    try {
      setPing(await pingSidecar());
    } catch (e) {
      setPing(null);
      setEnvError(String(e));
    } finally {
      setPinging(false);
    }
  }, []);

  return (
    <>
      {envError && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{envError}</p>
        </div>
      )}

      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">サイドカー接続テスト</span>
          {ping && (
            <span className="panel__freshness">
              最終確認: {formatJst(ping.checked_at)}
            </span>
          )}
        </div>
        <button className="btn" onClick={onPing} disabled={pinging}>
          {pinging ? 'ping 実行中…' : 'ping を実行'}
        </button>
        {ping && (
          <>
            <div style={{ marginTop: 14 }}>
              {ping.reachable ? (
                <span className="badge badge--ok">疎通 OK</span>
              ) : (
                <span className="badge badge--err">疎通失敗</span>
              )}
            </div>
            <dl className="kv">
              <dt>HTTP ステータス</dt>
              <dd>{ping.http_status}</dd>
              <dt>サイドカー状態</dt>
              <dd>{ping.health?.status ?? '—'}</dd>
              <dt>ポート</dt>
              <dd>{ping.port}</dd>
              <dt>往復時間</dt>
              <dd>{ping.round_trip_ms} ms</dd>
            </dl>
          </>
        )}
      </section>

      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">ローカルデータベース</span>
          {db && (
            <span className="panel__freshness">
              初期化: {formatJst(db.initialized_at)}
            </span>
          )}
        </div>
        {db ? (
          <dl className="kv">
            <dt>DuckDB</dt>
            <dd>
              {db.duckdb_exists ? '✓ ' : '✗ '}
              {db.duckdb_path}
            </dd>
            <dt>SQLite</dt>
            <dd>
              {db.sqlite_exists ? '✓ ' : '✗ '}
              {db.sqlite_path}
            </dd>
          </dl>
        ) : (
          <span className="badge badge--idle">未取得</span>
        )}
      </section>
    </>
  );
}
