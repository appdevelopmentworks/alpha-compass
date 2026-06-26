'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  getComposite,
  getUsMarket,
  getBrief,
  getAlerts,
  isTauri,
  type CompositeResult,
  type UsMarket,
  type Brief,
  type Alert,
} from '@/lib/ipc';
import { formatJst, fmtNum, fmtPct, signClass } from '@/lib/format';

const REGIME_COLOR: Record<string, string> = {
  strong_risk_off: 'var(--err)',
  risk_off: '#e07a3f',
  neutral: 'var(--warn)',
  risk_on: '#5bbf6a',
  strong_risk_on: 'var(--ok)',
};

function regimeColor(key: string): string {
  return REGIME_COLOR[key] ?? 'var(--text-dim)';
}

export default function HomeView() {
  const [comp, setComp] = useState<CompositeResult | null>(null);
  const [us, setUs] = useState<UsMarket | null>(null);
  const [brief, setBrief] = useState<Brief | null>(null);
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      setLoading(false);
      return;
    }
    try {
      const [c, u] = await Promise.all([getComposite(), getUsMarket()]);
      setComp(c);
      setUs(u);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
    // Brief + alerts are best-effort (don't block the headline).
    getBrief().then(setBrief).catch(() => {});
    getAlerts(5).then(setAlerts).catch(() => {});
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const available = comp?.components.filter((c) => c.available) ?? [];
  const maxContrib = Math.max(
    0.0001,
    ...available.map((c) => Math.abs(c.contribution)),
  );

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          ホーム（地合い）
        </span>
        <div className="toolbar__right">
          {comp && (
            <span className="panel__freshness">算出: {formatJst(comp.ts)}</span>
          )}
          <button className="btn" onClick={load} disabled={loading}>
            {loading ? '…' : '再計算'}
          </button>
        </div>
      </div>

      {error && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{error}</p>
        </div>
      )}

      {comp && (
        <section className="panel">
          {/* Score headline */}
          <div className="score">
            <div className="score__big" style={{ color: regimeColor(comp.regime_key) }}>
              {fmtNum(comp.score, 0)}
            </div>
            <div className="score__meta">
              <div
                className="score__regime"
                style={{ color: regimeColor(comp.regime_key) }}
              >
                {comp.regime_label}
              </div>
              <div className="score__sub">
                地合いスコア（0–100）・カバレッジ {fmtNum(comp.coverage * 100, 0)}%
              </div>
            </div>
          </div>

          {/* 0-100 scale bar */}
          <div className="scale">
            <div className="scale__bar" />
            <div
              className="scale__marker"
              style={{ left: `${comp.score}%` }}
              title={`score ${comp.score}`}
            />
            <div className="scale__ticks">
              <span>リスクオフ</span>
              <span>中立</span>
              <span>リスクオン</span>
            </div>
          </div>

          {comp.notes.length > 0 && (
            <p className="note">{comp.notes.join(' ')}</p>
          )}
        </section>
      )}

      {/* Contribution breakdown */}
      {comp && (
        <section className="panel">
          <div className="panel__head">
            <span className="panel__title">寄与度（説明可能な内訳）</span>
            <span className="panel__freshness">重み: 設定値（§8 既定）</span>
          </div>
          <div className="sectors">
            {comp.components.map((c) => {
              const width = c.available
                ? (Math.abs(c.contribution) / maxContrib) * 50
                : 0;
              return (
                <div className="sector-row" key={c.name}>
                  <span className="sector-row__name" title={c.note ?? ''}>
                    {c.label}
                  </span>
                  <div className="sector-row__track">
                    {c.available ? (
                      <div
                        className={`sector-row__bar ${signClass(c.contribution)}`}
                        style={{
                          width: `${width}%`,
                          marginLeft:
                            c.contribution >= 0 ? '50%' : `${50 - width}%`,
                        }}
                      />
                    ) : null}
                  </div>
                  <span className={`sector-row__val ${signClass(c.contribution)}`}>
                    {c.available ? fmtNum(c.contribution, 3) : '未取得'}
                  </span>
                  <span className="sector-row__ret">
                    {fmtNum(c.weight * 100, 0)}%
                  </span>
                </div>
              );
            })}
          </div>
          <p className="note">
            各寄与度 = 有効重み × 正規化値（±1）。本スコアは状況把握のための透明な合成指標であり、売買推奨ではありません。
          </p>
        </section>
      )}

      {/* Mini key indices */}
      {us && us.indices.length > 0 && (
        <section className="panel">
          <div className="panel__head">
            <span className="panel__title">主要指数</span>
            <span className="panel__freshness">出所: yfinance</span>
          </div>
          <div className="grid grid--4">
            {us.indices.map((ix) => (
              <div className="metric" key={ix.code}>
                <div className="metric__label">{ix.code}</div>
                <div className="metric__value">{fmtNum(ix.value, 2)}</div>
                <div className={`metric__sub ${signClass(ix.change_pct)}`}>
                  {fmtPct(ix.change_pct)}
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Daily brief + latest alerts */}
      <div className="grid grid--2-loose">
        <section className="panel">
          <div className="panel__head">
            <span className="panel__title">当日ブリーフ</span>
            {brief && (
              <span className="panel__freshness">
                {brief.tier === 'tier1_rule'
                  ? 'ルール生成'
                  : brief.tier === 'tier2_local'
                    ? 'ローカルLLM'
                    : 'Claude'}
              </span>
            )}
          </div>
          {brief ? (
            <p className="note" style={{ marginTop: 0, fontSize: 13 }}>
              {brief.text}
            </p>
          ) : (
            <span className="badge badge--idle">生成中…</span>
          )}
        </section>
        <section className="panel">
          <div className="panel__head">
            <span className="panel__title">最新アラート</span>
          </div>
          {alerts.length > 0 ? (
            <ul className="feed">
              {alerts.map((a) => (
                <li className="feed__item" key={a.id}>
                  <div className="feed__head">
                    <span className="feed__title">{a.title}</span>
                    <span
                      className={
                        a.severity === 'high' ? 'badge badge--err' : 'badge badge--idle'
                      }
                    >
                      {a.severity === 'high' ? '高' : '中'}
                    </span>
                  </div>
                  <div className="feed__meta">{formatJst(a.ts)}</div>
                </li>
              ))}
            </ul>
          ) : (
            <p className="note" style={{ marginTop: 0 }}>
              複数シグナル一致時のみ発火。現在は平常です。
            </p>
          )}
        </section>
      </div>
    </div>
  );
}
