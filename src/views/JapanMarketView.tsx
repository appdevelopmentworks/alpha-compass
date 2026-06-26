'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  getJpMarket,
  getWatchlist,
  setWatchlist as saveWatchlist,
  refresh as refreshSources,
  isTauri,
  type JpMarket,
  type WatchItem,
} from '@/lib/ipc';
import { FreshnessChips } from '@/components/Freshness';
import { formatJst, fmtNum, fmtInt, fmtPct, signClass } from '@/lib/format';

const INDEX_LABEL: Record<string, string> = {
  N225: '日経225',
  TOPIX: 'TOPIX',
  TOPIX_ETF: 'TOPIX連動ETF(1306)',
};

const INVESTOR_LABEL: Record<string, string> = {
  foreigners: '海外投資家',
  individuals: '個人',
  trust_banks: '信託銀行',
  investment_trusts: '投資信託',
  business_corps: '事業法人',
};

export default function JapanMarketView() {
  const [data, setData] = useState<JpMarket | null>(null);
  const [watch, setWatch] = useState<WatchItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newSym, setNewSym] = useState('');
  const [newLabel, setNewLabel] = useState('');

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      setLoading(false);
      return;
    }
    try {
      const [m, w] = await Promise.all([getJpMarket(), getWatchlist()]);
      setData(m);
      setWatch(w);
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

  const onRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await refreshSources();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  }, [load]);

  const addWatch = useCallback(async () => {
    const symbol = newSym.trim();
    if (!symbol) return;
    const next = [
      ...watch,
      { symbol, label: newLabel.trim() || symbol, market: 'JP' },
    ];
    await saveWatchlist(next);
    setWatch(next);
    setNewSym('');
    setNewLabel('');
  }, [newSym, newLabel, watch]);

  const removeWatch = useCallback(
    async (symbol: string) => {
      const next = watch.filter((w) => w.symbol !== symbol);
      await saveWatchlist(next);
      setWatch(next);
    },
    [watch],
  );

  const gap = data?.futures_gap;

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          日本市場
        </span>
        <div className="toolbar__right">
          <button className="btn" onClick={onRefresh} disabled={refreshing}>
            {refreshing ? '更新中…（最大30秒）' : '更新'}
          </button>
        </div>
      </div>

      {error && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{error}</p>
        </div>
      )}

      {data && <FreshnessChips metas={data.freshness} />}
      {loading && <div className="panel">読み込み中…</div>}

      {data && (
        <>
          {/* Indices + FX */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">指数・為替</span>
              <span className="panel__freshness">出所: yfinance / fx</span>
            </div>
            <div className="grid grid--4">
              {data.indices.map((ix) => (
                <div className="metric" key={ix.code}>
                  <div className="metric__label">
                    {INDEX_LABEL[ix.code] ?? ix.code}
                  </div>
                  <div className="metric__value">{fmtNum(ix.value, 2)}</div>
                  <div className={`metric__sub ${signClass(ix.change_pct)}`}>
                    {fmtPct(ix.change_pct)}
                  </div>
                </div>
              ))}
              {data.fx.map((fx) => (
                <div className="metric" key={fx.pair}>
                  <div className="metric__label">{fx.pair}</div>
                  <div className="metric__value">{fmtNum(fx.rate, 2)}</div>
                  <div className={`metric__sub ${signClass(fx.change_pct)}`}>
                    {fmtPct(fx.change_pct)}
                  </div>
                </div>
              ))}
            </div>
          </section>

          {/* Futures gap hint */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">先物・寄り付きギャップ示唆</span>
              <span className="panel__freshness">出所: CME 日経先物 (NKD)</span>
            </div>
            {gap && gap.gap_pct !== null ? (
              <>
                <div className="grid grid--3">
                  <div className="metric">
                    <div className="metric__label">日経先物 (NKD)</div>
                    <div className="metric__value">
                      {fmtNum(gap.futures_value, 0)}
                    </div>
                    <div className="metric__sub">{gap.futures_ts ?? '—'}</div>
                  </div>
                  <div className="metric">
                    <div className="metric__label">日経225 直近終値</div>
                    <div className="metric__value">
                      {fmtNum(gap.spot_prev_close, 0)}
                    </div>
                  </div>
                  <div className="metric">
                    <div className="metric__label">想定ギャップ</div>
                    <div
                      className={`metric__value ${signClass(gap.gap_pct)}`}
                    >
                      {fmtPct(gap.gap_pct)}
                    </div>
                  </div>
                </div>
                <p className="note">
                  夜間の CME 日経先物は、寄り付きで{' '}
                  <span className={signClass(gap.gap_pct)}>
                    {gap.gap_pct >= 0 ? '上' : '下'}方向に {fmtPct(gap.gap_pct)}
                  </span>{' '}
                  のギャップを示唆（あくまで参考）。
                </p>
              </>
            ) : (
              <span className="badge badge--idle">データがありません。</span>
            )}
          </section>

          {/* Investor flows (J-Quants) */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">投資部門別売買動向（海外勢フロー）</span>
              <span className="panel__freshness">出所: J-Quants（週次・遅延）</span>
            </div>
            {!data.jquants_available && (
              <p className="note">
                J-Quants 資格情報が未設定のため、投資部門別・信用・空売りは未取得です。無料プランは約12週の配信遅延があります（設定で登録予定）。
              </p>
            )}
            {data.investor_flows.length > 0 ? (
              <table className="table">
                <thead>
                  <tr>
                    <th>部門</th>
                    <th className="num">ネット</th>
                    <th className="num">買い</th>
                    <th className="num">売り</th>
                    <th>週</th>
                  </tr>
                </thead>
                <tbody>
                  {data.investor_flows.map((f) => (
                    <tr key={f.investor_type}>
                      <td>{INVESTOR_LABEL[f.investor_type] ?? f.investor_type}</td>
                      <td className={`num ${signClass(f.net)}`}>{fmtInt(f.net)}</td>
                      <td className="num">{fmtInt(f.buy)}</td>
                      <td className="num">{fmtInt(f.sell)}</td>
                      <td>{f.week_ending}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <span className="badge badge--idle">未取得</span>
            )}
          </section>

          {/* Short selling (J-Quants) */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">空売り比率（業種別）</span>
              <span className="panel__freshness">出所: J-Quants</span>
            </div>
            {data.short_selling.length > 0 ? (
              <table className="table">
                <thead>
                  <tr>
                    <th>区分</th>
                    <th className="num">空売り比率</th>
                    <th>日付</th>
                  </tr>
                </thead>
                <tbody>
                  {data.short_selling.map((s) => (
                    <tr key={`${s.market}-${s.date}`}>
                      <td>{s.market}</td>
                      <td className="num">{fmtNum(s.short_ratio, 2)}</td>
                      <td>{s.date}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <span className="badge badge--idle">未取得</span>
            )}
          </section>

          {/* Watchlist */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">ウォッチリスト</span>
            </div>
            {watch.length > 0 ? (
              <table className="table">
                <thead>
                  <tr>
                    <th>コード</th>
                    <th>銘柄名</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {watch.map((w) => (
                    <tr key={w.symbol}>
                      <td>{w.symbol}</td>
                      <td>{w.label}</td>
                      <td className="num">
                        <button
                          className="nav__item"
                          onClick={() => removeWatch(w.symbol)}
                        >
                          削除
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <span className="badge badge--idle">登録なし</span>
            )}
            <div className="wl-add">
              <input
                className="wl-input"
                placeholder="コード（例 7203.T）"
                value={newSym}
                onChange={(e) => setNewSym(e.target.value)}
              />
              <input
                className="wl-input"
                placeholder="銘柄名（任意）"
                value={newLabel}
                onChange={(e) => setNewLabel(e.target.value)}
              />
              <button className="btn" onClick={addWatch}>
                追加
              </button>
            </div>
            <p className="note">
              ウォッチリストは SQLite（Rust 管理）に保存されます。個別銘柄の株価表示は J-Quants 接続後に拡充予定。
            </p>
          </section>
        </>
      )}
    </div>
  );
}
