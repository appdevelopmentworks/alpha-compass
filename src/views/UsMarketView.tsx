'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  getUsMarket,
  refresh as refreshSources,
  isTauri,
  type UsMarket,
  type SourceMeta,
  type MetricPoint,
} from '@/lib/ipc';
import { formatJst, fmtNum, fmtInt, fmtPct, fmtPctFrac, signClass } from '@/lib/format';

const STATUS_LABEL: Record<string, string> = {
  ok: '最新',
  error: 'エラー',
  unavailable: '未取得',
  never: '未取得',
};

function statusBadgeClass(status: string): string {
  if (status === 'ok') return 'badge badge--ok';
  if (status === 'error') return 'badge badge--err';
  return 'badge badge--idle';
}

function FreshnessChip({ meta }: { meta: SourceMeta }) {
  return (
    <div className="chip" title={meta.detail ?? ''}>
      <span className="chip__src">{meta.source}</span>
      <span className={statusBadgeClass(meta.status)}>
        {STATUS_LABEL[meta.status] ?? meta.status}
      </span>
      <span className="chip__time">
        {meta.last_fetched_at ? formatJst(meta.last_fetched_at) : '—'}
      </span>
    </div>
  );
}

function Metric({
  label,
  point,
  unit,
  digits = 2,
}: {
  label: string;
  point: MetricPoint | null;
  unit?: string;
  digits?: number;
}) {
  return (
    <div className="metric">
      <div className="metric__label">{label}</div>
      <div className="metric__value">
        {point ? `${fmtNum(point.value, digits)}${unit ?? ''}` : '—'}
      </div>
      <div className="metric__sub">
        {point ? `as of ${point.date}` : '未取得'}
      </div>
    </div>
  );
}

export default function UsMarketView() {
  const [data, setData] = useState<UsMarket | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [refreshedAt, setRefreshedAt] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      setLoading(false);
      return;
    }
    try {
      setData(await getUsMarket());
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
    if (!isTauri()) return;
    setRefreshing(true);
    try {
      const summary = await refreshSources();
      setRefreshedAt(summary.at);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  }, [load]);

  const sectors = data?.sectors ?? [];
  const maxRel = Math.max(
    0.0001,
    ...sectors.map((s) => Math.abs(s.rel_strength ?? 0)),
  );

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          米国市場
        </span>
        <div className="toolbar__right">
          {refreshedAt && (
            <span className="panel__freshness">
              更新: {formatJst(refreshedAt)}
            </span>
          )}
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

      {/* Freshness strip */}
      {data && (
        <div className="chips">
          {data.freshness.map((m) => (
            <FreshnessChip key={m.source} meta={m} />
          ))}
        </div>
      )}

      {loading && <div className="panel">読み込み中…</div>}

      {data && (
        <>
          {/* Indices */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">主要指数</span>
              <span className="panel__freshness">出所: yfinance</span>
            </div>
            {data.indices.length === 0 ? (
              <span className="badge badge--idle">
                データがありません。「更新」を押してください。
              </span>
            ) : (
              <div className="grid grid--4">
                {data.indices.map((ix) => (
                  <div className="metric" key={ix.code}>
                    <div className="metric__label">{ix.code}</div>
                    <div className="metric__value">{fmtNum(ix.value, 2)}</div>
                    <div className={`metric__sub ${signClass(ix.change_pct)}`}>
                      {fmtPct(ix.change_pct)}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>

          {/* Rates / credit / volatility */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">金利・クレジット・ボラティリティ</span>
              <span className="panel__freshness">出所: FRED / yfinance</span>
            </div>
            <div className="grid grid--3">
              <Metric label="米2年金利" point={data.rates.us2y} unit="%" />
              <Metric label="米10年金利" point={data.rates.us10y} unit="%" />
              <Metric label="2s10s スプレッド" point={data.rates.twos10s} unit="%" />
              <Metric label="HY OAS" point={data.rates.hy_oas} unit="%" />
              <Metric label="ドルインデックス (DXY)" point={data.rates.dxy} />
              <Metric label="VIX" point={data.rates.vix} />
            </div>
            {!data.rates.us2y && !data.rates.hy_oas && (
              <p className="note">
                2年金利・HY OAS は FRED API キーが必要です（設定で登録予定）。
              </p>
            )}
          </section>

          {/* Sector relative strength */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">セクター相対強弱（対 SPY・約1か月）</span>
              <span className="panel__freshness">出所: yfinance</span>
            </div>
            {sectors.length === 0 ? (
              <span className="badge badge--idle">データがありません。</span>
            ) : (
              <div className="sectors">
                {sectors.map((s) => {
                  const rel = s.rel_strength ?? 0;
                  const width = (Math.abs(rel) / maxRel) * 50; // % of half-width
                  return (
                    <div className="sector-row" key={s.sector}>
                      <span className="sector-row__name">{s.sector}</span>
                      <div className="sector-row__track">
                        <div
                          className={`sector-row__bar ${signClass(rel)}`}
                          style={{
                            width: `${width}%`,
                            marginLeft: rel >= 0 ? '50%' : `${50 - width}%`,
                          }}
                        />
                      </div>
                      <span className={`sector-row__val ${signClass(rel)}`}>
                        {fmtPctFrac(s.rel_strength)}
                      </span>
                      <span className="sector-row__ret">{fmtPctFrac(s.ret)}</span>
                    </div>
                  );
                })}
              </div>
            )}
          </section>

          {/* Breadth */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">市場ブレッドス</span>
              <span className="panel__freshness">出所: yfinance（簡易）</span>
            </div>
            {data.breadth ? (
              <>
                <div className="grid grid--4">
                  <div className="metric">
                    <div className="metric__label">200日線上 比率</div>
                    <div className="metric__value">
                      {fmtPctFrac(data.breadth.pct_above_200dma, 1)}
                    </div>
                    <div className="metric__sub">
                      母集団 {fmtInt(data.breadth.universe)} 銘柄
                    </div>
                  </div>
                  <div className="metric">
                    <div className="metric__label">騰落（上げ/下げ）</div>
                    <div className="metric__value">
                      <span className="up">{fmtInt(data.breadth.advancers)}</span>
                      {' / '}
                      <span className="down">{fmtInt(data.breadth.decliners)}</span>
                    </div>
                    <div className="metric__sub">{data.breadth.date}</div>
                  </div>
                  <div className="metric">
                    <div className="metric__label">新高値 / 新安値</div>
                    <div className="metric__value">
                      <span className="up">{fmtInt(data.breadth.new_highs)}</span>
                      {' / '}
                      <span className="down">{fmtInt(data.breadth.new_lows)}</span>
                    </div>
                    <div className="metric__sub">52週基準</div>
                  </div>
                </div>
                <p className="note">
                  ※ ブレッドスは大型株サンプル基準の簡易指標です（本格的な市場全体ブレッドスは後続で拡充）。
                </p>
              </>
            ) : (
              <span className="badge badge--idle">データがありません。</span>
            )}
          </section>

          {/* COT */}
          <section className="panel">
            <div className="panel__head">
              <span className="panel__title">CFTC COT（投機筋ネット）</span>
              <span className="panel__freshness">出所: CFTC（週次）</span>
            </div>
            {data.cot.length === 0 ? (
              <span className="badge badge--idle">データがありません。</span>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>市場</th>
                    <th className="num">投機筋ネット</th>
                    <th className="num">前週比</th>
                    <th>報告日</th>
                  </tr>
                </thead>
                <tbody>
                  {data.cot.map((c) => {
                    const delta =
                      c.noncomm_net !== null && c.noncomm_net_prev !== null
                        ? c.noncomm_net - c.noncomm_net_prev
                        : null;
                    return (
                      <tr key={c.market}>
                        <td>{c.market}</td>
                        <td className={`num ${signClass(c.noncomm_net)}`}>
                          {fmtInt(c.noncomm_net)}
                        </td>
                        <td className={`num ${signClass(delta)}`}>
                          {delta === null
                            ? '—'
                            : `${delta > 0 ? '+' : ''}${fmtInt(delta)}`}
                        </td>
                        <td>{c.date ?? '—'}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </section>
        </>
      )}
    </div>
  );
}
