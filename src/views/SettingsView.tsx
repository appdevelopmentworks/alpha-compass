'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  getSettings,
  setSetting,
  getCredentialStatus,
  setCredential,
  isTauri,
  type CredentialStatus,
} from '@/lib/ipc';

const CRED_LABEL: Record<string, string> = {
  fred: 'FRED（米国マクロ・金利/クレジット）',
  jquants: 'J-Quants API キー（v2・ダッシュボードで発行）',
  anthropic: 'Anthropic（AI Tier3 ブリーフ）',
  edinet: 'EDINET（開示）',
};

const WEIGHT_KEYS: { key: string; label: string }[] = [
  { key: 'us_trend', label: '米国株トレンド' },
  { key: 'breadth', label: 'ブレッドス' },
  { key: 'vix', label: 'ボラティリティ' },
  { key: 'credit', label: 'クレジット' },
  { key: 'rate', label: '米10年金利' },
  { key: 'usdjpy', label: 'ドル円' },
  { key: 'foreign_flow', label: '海外勢フロー' },
];

export default function SettingsView() {
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [creds, setCreds] = useState<CredentialStatus[]>([]);
  const [credInputs, setCredInputs] = useState<Record<string, string>>({});
  const [weights, setWeights] = useState<Record<string, number>>({});
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      return;
    }
    try {
      const [kv, cs] = await Promise.all([getSettings(), getCredentialStatus()]);
      const map: Record<string, string> = {};
      kv.forEach((s) => (map[s.key] = s.value));
      setSettings(map);
      setCreds(cs);
      try {
        setWeights(JSON.parse(map['composite_weights'] ?? '{}'));
      } catch {
        setWeights({});
      }
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const flash = (m: string) => {
    setMsg(m);
    setTimeout(() => setMsg(null), 2500);
  };

  const saveCred = useCallback(
    async (source: string) => {
      await setCredential(source, credInputs[source] ?? '');
      setCredInputs((p) => ({ ...p, [source]: '' }));
      await load();
      flash(`${source} の資格情報を更新しました（サイドカー再起動後に反映）。`);
    },
    [credInputs, load],
  );

  const saveWeights = useCallback(async () => {
    await setSetting('composite_weights', JSON.stringify(weights));
    flash('コンポジット重みを保存しました。');
  }, [weights]);

  const saveSetting = useCallback(async (key: string, value: string) => {
    await setSetting(key, value);
    setSettings((p) => ({ ...p, [key]: value }));
    flash(`${key} を保存しました。`);
  }, []);

  const weightSum = Object.values(weights).reduce((a, b) => a + (Number(b) || 0), 0);

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          設定
        </span>
        {msg && <span className="badge badge--ok">{msg}</span>}
      </div>

      {error && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{error}</p>
        </div>
      )}

      {/* Credentials */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">資格情報（OS キーチェーン保存）</span>
        </div>
        {creds.map((c) => (
          <div className="cred-row" key={c.source}>
            <div className="cred-row__label">
              {CRED_LABEL[c.source] ?? c.source}
              <span className={c.configured ? 'badge badge--ok' : 'badge badge--idle'}>
                {c.configured ? '設定済み' : '未設定'}
              </span>
            </div>
            <input
              className="wl-input"
              type={c.source === 'jquants_mail' ? 'text' : 'password'}
              placeholder={
                c.source === 'jquants_mail'
                  ? 'メールアドレスを入力（空で削除）'
                  : 'トークン / キーを入力（空で削除）'
              }
              value={credInputs[c.source] ?? ''}
              onChange={(e) =>
                setCredInputs((p) => ({ ...p, [c.source]: e.target.value }))
              }
            />
            <button className="btn" onClick={() => saveCred(c.source)}>
              保存
            </button>
          </div>
        ))}
        <p className="note">
          トークンは平文保存せず OS キーチェーンに格納します。サイドカーには起動時のみ受け渡されるため、保存後はアプリ再起動で反映されます。
        </p>
      </section>

      {/* Composite weights */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">地合いコンポジットの重み（§8）</span>
          <span className="panel__freshness">
            合計 {weightSum.toFixed(2)}（利用可能分で自動再正規化）
          </span>
        </div>
        <div className="grid grid--3">
          {WEIGHT_KEYS.map((w) => (
            <div className="metric" key={w.key}>
              <div className="metric__label">{w.label}</div>
              <input
                className="wl-input"
                type="number"
                step="0.05"
                min="0"
                max="1"
                value={weights[w.key] ?? 0}
                onChange={(e) =>
                  setWeights((p) => ({ ...p, [w.key]: Number(e.target.value) }))
                }
              />
            </div>
          ))}
        </div>
        <div className="wl-add">
          <button className="btn" onClick={saveWeights}>
            重みを保存
          </button>
        </div>
        <div className="kv" style={{ marginTop: 16 }}>
          <dt>金利シグナルの符号</dt>
          <dd>
            <select
              className="wl-input"
              value={settings['rate_sign'] ?? '-1'}
              onChange={(e) => saveSetting('rate_sign', e.target.value)}
            >
              <option value="-1">-1（金利上昇＝リスクオフ寄り）</option>
              <option value="1">+1（金利上昇＝景気期待・リスクオン寄り）</option>
            </select>
          </dd>
        </div>
      </section>

      {/* Alert thresholds */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">アラート（複数ソース照合）</span>
        </div>
        <div className="grid grid--3">
          <div className="metric">
            <div className="metric__label">最小一致系統数</div>
            <input
              className="wl-input"
              type="number"
              min="2"
              max="7"
              value={settings['alert_min_families'] ?? '3'}
              onChange={(e) => saveSetting('alert_min_families', e.target.value)}
            />
          </div>
          <div className="metric">
            <div className="metric__label">発火しきい値 |n|</div>
            <input
              className="wl-input"
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={settings['alert_threshold'] ?? '0.3'}
              onChange={(e) => saveSetting('alert_threshold', e.target.value)}
            />
          </div>
        </div>
        <p className="note">
          独立した N 系統以上のシグナルが同方向に一致したときのみアラートを発火します（単一ソースでは鳴らさない）。
        </p>
      </section>

      {/* AI cost control */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">AI 要約・当日ブリーフ（コスト制御）</span>
        </div>
        <div className="kv">
          <dt>AI モード</dt>
          <dd>
            <select
              className="wl-input"
              value={settings['ai_max_tier'] ?? 'local'}
              onChange={(e) => saveSetting('ai_max_tier', e.target.value)}
            >
              <option value="rule">ルールのみ（無料・LLM不使用）</option>
              <option value="local">ローカルLLM優先（Claudeは使わない）</option>
              <option value="claude">Claude 許可（ローカル→Claude）</option>
            </select>
          </dd>
        </div>
        <p className="note">
          ブリーフは最大1時間に1回だけ再生成します（5分毎の課金を防止）。「ローカルLLM優先」は
          RTX 5090 のローカルサーバー（OpenAI 互換・<code>LOCAL_LLM_ENDPOINT</code>）が起動していれば自動で使用し、
          無ければルール要約にフォールバック（Claude は呼びません）。ニュース要約は常に Claude を使いません。
        </p>
      </section>
    </div>
  );
}
