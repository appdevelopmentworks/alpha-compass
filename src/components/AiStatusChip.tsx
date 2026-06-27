'use client';

import { useCallback, useEffect, useState } from 'react';
import { getAiStatus, isTauri, type AiStatus } from '@/lib/ipc';

const DOT_COLOR: Record<string, string> = {
  local: 'var(--ok)',
  claude: 'var(--accent-strong)',
  rule: 'var(--text-dim)',
};

/** Top-right indicator of which AI provider is currently active. */
export default function AiStatusChip() {
  const [status, setStatus] = useState<AiStatus | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setStatus(await getAiStatus());
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 30000); // re-check every 30s
    return () => clearInterval(id);
  }, [refresh]);

  if (!status) return null;

  const title =
    status.provider === 'local'
      ? `ローカルLLM\nエンドポイント: ${status.local_endpoint ?? '—'}\nモデル: ${status.local_model ?? '—'}`
      : status.provider === 'claude'
        ? 'Claude API（Tier3）'
        : 'LLM 不使用（ルール要約）';

  return (
    <button
      className="ai-chip"
      onClick={refresh}
      title={`${title}\n（クリックで再確認）`}
    >
      <span
        className="ai-chip__dot"
        style={{ background: DOT_COLOR[status.provider] ?? 'var(--text-dim)' }}
      />
      <span className="ai-chip__label">AI: {status.label}</span>
    </button>
  );
}
