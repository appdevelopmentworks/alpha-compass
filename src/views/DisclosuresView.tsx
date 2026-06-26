'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  getNews,
  getDisclosures,
  getCalendar,
  refresh as refreshSources,
  isTauri,
  type NewsItem,
  type DisclosureItem,
  type CalendarEvent,
} from '@/lib/ipc';
import { formatJst } from '@/lib/format';

const TIER_BADGE: Record<string, { label: string; cls: string }> = {
  tier1_rule: { label: 'ルール', cls: 'badge badge--idle' },
  tier2_local: { label: 'ローカルLLM', cls: 'badge badge--ok' },
  tier3_claude: { label: 'Claude', cls: 'badge badge--ok' },
};

function TierBadge({ tier }: { tier: string | null }) {
  if (!tier) return null;
  const t = TIER_BADGE[tier] ?? { label: tier, cls: 'badge badge--idle' };
  return <span className={t.cls} title={`生成: ${tier}`}>{t.label}</span>;
}

const EVENT_LABEL: Record<string, string> = { fomc: 'FOMC', boj: '日銀', econ: '経済指標', earnings: '決算' };

export default function DisclosuresView() {
  const [news, setNews] = useState<NewsItem[]>([]);
  const [disc, setDisc] = useState<DisclosureItem[]>([]);
  const [cal, setCal] = useState<CalendarEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sourceFilter, setSourceFilter] = useState<string>('all');

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError('`npm run tauri dev` で起動してください。');
      setLoading(false);
      return;
    }
    try {
      const [n, d, c] = await Promise.all([
        getNews(60),
        getDisclosures(),
        getCalendar(8),
      ]);
      setNews(n);
      setDisc(d);
      setCal(c);
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
      await refreshSources('news');
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  }, [load]);

  const sources = useMemo(
    () => ['all', ...Array.from(new Set(news.map((n) => n.source)))],
    [news],
  );
  const shownNews =
    sourceFilter === 'all' ? news : news.filter((n) => n.source === sourceFilter);

  return (
    <div>
      <div className="toolbar">
        <span className="app__title" style={{ fontSize: 18 }}>
          開示 / ニュース
        </span>
        <div className="toolbar__right">
          <select
            className="wl-input"
            value={sourceFilter}
            onChange={(e) => setSourceFilter(e.target.value)}
          >
            {sources.map((s) => (
              <option key={s} value={s}>
                {s === 'all' ? '全ソース' : s}
              </option>
            ))}
          </select>
          <button className="btn" onClick={onRefresh} disabled={refreshing}>
            {refreshing ? '更新中…' : '更新'}
          </button>
        </div>
      </div>

      {error && (
        <div className="panel">
          <span className="badge badge--err">注意</span>
          <p className="note">{error}</p>
        </div>
      )}
      {loading && <div className="panel">読み込み中…</div>}

      {/* Upcoming calendar */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">予定（FOMC / 日銀 ほか）</span>
        </div>
        {cal.length > 0 ? (
          <div className="chips">
            {cal.map((e) => (
              <div className="chip" key={e.id}>
                <span className="chip__src">{EVENT_LABEL[e.type] ?? e.type}</span>
                <span className="chip__time">{e.datetime_jst.slice(0, 16).replace('T', ' ')}</span>
              </div>
            ))}
          </div>
        ) : (
          <span className="badge badge--idle">予定なし</span>
        )}
      </section>

      {/* News feed */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">ニュース（AI要約付き）</span>
          <span className="panel__freshness">{shownNews.length} 件</span>
        </div>
        {shownNews.length === 0 ? (
          <span className="badge badge--idle">データがありません。「更新」を押してください。</span>
        ) : (
          <ul className="feed">
            {shownNews.map((n) => (
              <li className="feed__item" key={n.id}>
                <div className="feed__head">
                  <span className="feed__title">{n.title}</span>
                  <TierBadge tier={n.summarized_tier} />
                </div>
                {n.summary && <div className="feed__summary">{n.summary}</div>}
                <div className="feed__meta">
                  {n.source} ・ {formatJst(n.datetime)}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Disclosures (EDINET/TDnet) */}
      <section className="panel">
        <div className="panel__head">
          <span className="panel__title">適時開示 / EDINET</span>
        </div>
        {disc.length === 0 ? (
          <p className="note">
            EDINET API キー / TDnet 取得方式が未設定のため、開示は未取得です（設定で登録予定）。ニュースは上に表示されています。
          </p>
        ) : (
          <ul className="feed">
            {disc.map((d) => (
              <li className="feed__item" key={d.id}>
                <div className="feed__head">
                  <span className="feed__title">{d.title}</span>
                  <TierBadge tier={d.summarized_tier} />
                </div>
                {d.summary && <div className="feed__summary">{d.summary}</div>}
                <div className="feed__meta">
                  {d.source}
                  {d.company_code ? ` ・ ${d.company_code}` : ''} ・ {formatJst(d.datetime)}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
