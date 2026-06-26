'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  getCrossMarket,
  isTauri,
  type CrossMarket,
} from '@/lib/ipc';
import { fmtNum, signClass } from '@/lib/format';

export default function CrossMarketView() {
  const [data, setData] = useState<CrossMarket | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      setLoading(false);
      return;
    }
    try {
      setData(await getCrossMarket());
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          クロスマーケット（米国→日本）
        </span>
        <button className="btn" onClick={load} disabled={loading}>
          {loading ? '…' : '再計算'}
        </button>
      </div>

      {error && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{error}</p>
        </div>
      )}

      {data && (
        <>
          {/* Observed metrics */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">観測された前提</span>
            </div>
            <div className="grid grid--3">
              {data.metrics.map((m) => (
                <div className="metric" key={m.label}>
                  <div className="metric__label">{m.label}</div>
                  <div className={`metric__value ${signClass(m.value)}`}>
                    {m.value === null ? '—' : fmtNum(m.value, 2)}
                  </div>
                </div>
              ))}
            </div>
          </section>

          {/* Transmissions */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">想定される波及（示唆）</span>
            </div>
            {data.transmissions.length > 0 ? (
              <ul className="feed">
                {data.transmissions.map((t, i) => (
                  <li className="feed__item" key={i}>
                    <div className="feed__head">
                      <span className="feed__title">{t.driver}</span>
                    </div>
                    <div className="feed__summary">→ {t.path}</div>
                    <div className="feed__meta">{t.effect_note}</div>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="note">
                現在、ルールに一致する明確な波及シグナルはありません（前提が中立的）。
              </p>
            )}
            <p className="note">
              ※ これは予測ではなく、観測された前提から想定される<strong>傾斜の透明な注記</strong>です。ルールは設定で編集できます。
            </p>
          </section>
        </>
      )}
    </div>
  );
}
