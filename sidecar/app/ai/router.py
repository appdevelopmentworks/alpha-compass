"""AI cost-cascade router (architecture §10).

Routes to the cheapest tier that can do the job, in true cost order:
Tier1 rules (free) -> Tier2 local Qwen (GPU) -> Tier3 Claude (metered, gated).

`max_tier` caps how far the cascade may escalate, so the user controls cost:
- "rule"   : Tier1 only (no LLM, $0)
- "local"  : Tier1 -> Tier2 local (never Claude)   [default]
- "claude" : Tier1 -> Tier2 -> Tier3 Claude

Always returns the generating tier so the UI can show provenance.
"""

from __future__ import annotations

from app.ai import claude, local_qwen, rules

DEFAULT_MAX_TIER = "local"


def summarize(
    title: str, body: str = "", importance: int = 0, max_tier: str = DEFAULT_MAX_TIER
) -> tuple[str, str]:
    """Return (summary, tier). Escalates only as far as `max_tier` allows."""
    if max_tier == "rule":
        return rules.summarize(title, body), "tier1_rule"

    text_len = len(rules.clean_text(body))
    if text_len >= 60:
        if local_qwen.available():
            s = local_qwen.summarize(title, body)
            if s:
                return s, "tier2_local"
        if max_tier == "claude" and importance >= 2 and claude.available():
            s = claude.summarize(title, body)
            if s:
                return s, "tier3_claude"

    return rules.summarize(title, body), "tier1_rule"


def _rule_brief(payload: dict) -> str:
    score = payload.get("score")
    regime = payload.get("regime_label") or payload.get("regime") or "—"
    coverage = payload.get("coverage")
    headlines = payload.get("headlines") or []
    parts = [f"本日の地合いスコアは {score}（{regime}）です。"]
    if coverage is not None:
        parts.append(f"シグナル・カバレッジ {coverage}%。")
    if headlines:
        parts.append("注目ニュース: " + "／".join(headlines[:3]))
    return " ".join(parts)


def brief(payload: dict, max_tier: str = DEFAULT_MAX_TIER) -> tuple[str, str]:
    """Generate the daily brief, preferring local over Claude (cost order)."""
    if max_tier == "rule":
        return _rule_brief(payload), "tier1_rule"

    context = (
        f"地合いスコア: {payload.get('score')}（{payload.get('regime_label')}）\n"
        f"カバレッジ: {payload.get('coverage')}%\n"
        f"主要ニュース:\n" + "\n".join(f"- {h}" for h in (payload.get("headlines") or [])[:6])
    )

    # Tier2 local first (free/GPU), then Tier3 Claude only if allowed.
    if local_qwen.available():
        s = local_qwen.summarize("本日の注目点", context)
        if s:
            return s, "tier2_local"
    if max_tier == "claude" and claude.available():
        s = claude.brief(context)
        if s:
            return s, "tier3_claude"

    return _rule_brief(payload), "tier1_rule"
