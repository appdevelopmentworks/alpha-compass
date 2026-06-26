# alpha-compass — Claude Code キックオフプロンプト集

> 使い方（日本語）: 各セッションを Claude Code で始めるとき、対応するプロンプト（英語）をそのまま貼り付けてください。プロンプトは英語で書かれています（言語ポリシー: エージェント/プロンプトは英語、アプリ UI は日本語）。
> セッションの全体像は `docs/セッションプラン.md`、仕様は `docs/要件定義書.md` と `docs/アーキテクチャ設計書.md` を参照。

---

## 0. Master context (prepend to every session)

```
You are working on "alpha-compass", a Japan-first financial monitoring desktop app.

Read these first and treat them as the source of truth:
- docs/要件定義書.md (requirements)
- docs/アーキテクチャ設計書.md (architecture)
- docs/セッションプラン.md (session plan)

Hard rules:
1. CLEAN-ROOM. This project is inspired by the FINANCE part of "World Monitor" (AGPL-3.0, © Elie Habib) but must NOT copy or reference its code, assets, proto/config, or naming. Implement everything from scratch based on our own docs.
2. Stack is fixed: Tauri 2 (Rust) shell, Next.js with static export + TypeScript frontend, Python (managed by uv) FastAPI sidecar for data + AI, DuckDB (time-series/analytics) and SQLite (settings/state). Target OS: Windows (RTX 5090 available for local LLM).
3. Topology: Rust (Tauri Core) is the SOLE owner/writer of DuckDB and SQLite. The Python sidecar is a stateless fetch+AI service. The frontend NEVER calls external APIs directly — only Rust via `invoke` + events.
4. Language policy: app UI text is Japanese; code comments, identifiers, types, prompts, and commit messages are English.
5. Credentials (J-Quants, FRED, Anthropic, etc.) are NEVER stored in plaintext or committed. Use the OS keychain; .env is dev-only and git-ignored.
6. Every data point carries a freshness timestamp surfaced in the UI.

Work only within the scope of the current session. Do not implement future sessions. At the end, verify the session's acceptance criteria and write a short progress note into docs/.
```

---

## Session 0 — Project initialization

```
[Prepend Master context]

GOAL: Stand up a running skeleton with frontend ↔ Rust ↔ sidecar connectivity.

The project folder already contains a docs/ directory. Do not modify docs/ except to append a short progress note at the end. Create the rest of the structure per アーキテクチャ設計書 §13.

Tasks:
1. Initialize a Tauri 2 app with a Next.js (TypeScript) frontend using static export (next.config.js: output: 'export'). Produce src/ and src-tauri/.
2. Initialize the Python sidecar in sidecar/ using uv (FastAPI). Implement a GET /health endpoint.
3. Wire Rust (Tauri Core) to spawn the Python sidecar. Generate a random session token at startup and pass it to the sidecar via an environment variable; the sidecar must require it (401 on mismatch). Bind the sidecar to 127.0.0.1 only.
4. Add the duckdb and rusqlite crates to Rust and initialize empty DB files on first run.
5. Implement a minimal IPC round-trip: a "ping" button in the frontend → Rust command → sidecar /health → result shown in the UI.

ACCEPTANCE:
- The app launches in dev.
- "ping" succeeds end-to-end (frontend → Rust → sidecar → frontend).
- DuckDB and SQLite files are created.

Keep it minimal and correct. Confirm versions of Tauri 2 / Next.js are current. Then write the progress note.
```

---

## Session 1 — Data foundation + US core

```
[Prepend Master context]

GOAL: Build the persistence + scheduler skeleton and show the US market core with real data.

Tasks:
1. Store layer: implement DuckDB migrations for prices, indices, rates_macro, us_breadth, sector_perf, cot; SQLite for settings, source_meta. (Schema per アーキテクチャ設計書 §5.)
2. Scheduler skeleton (tokio): per-source cadence framework (§7), source_meta freshness updates, exponential backoff on rate limits.
3. Sidecar adapters: yfinance (US equities/indices), fred (rates/credit/DXY), cot. Normalize to the common schema (§5/§6). Rust receives JSON and upserts.
4. IPC: get_us_market(), get_freshness(), refresh(source?).
5. Frontend "US Market" view (minimal): indices, sector relative strength, breadth, rates/VIX/DXY, COT — each with a freshness label.

ACCEPTANCE:
- US core renders from real data and updates on manual refresh.
- Every panel shows its last-fetched time.

Write the progress note.
```

---

## Session 2 — Composite score + Home

```
[Prepend Master context]

GOAL: Compute the explainable market-regime composite and show the Home "conclusion" view.

Tasks:
1. Compute Engine (Rust, mostly pure functions): indicators (200DMA distance/slope, breadth aggregation), normalization (rolling z-score with winsorization), weighted sum, regime classification — exactly per アーキテクチャ設計書 §8.
2. Persist to composite_scores and signal_states; store per-component contributions as JSON.
3. IPC: get_composite().
4. Frontend "Home" view: large regime score + contribution bars + mini key indices + brief placeholder + alerts placeholder.
5. Unit tests for the composite computation.

ACCEPTANCE:
- Home shows a 0–100 score, a regime label, and each component's contribution.
- Weights are read from settings (defaults per §8).

Note: this is a situational-awareness metric, not a trade recommendation — keep it transparent and configurable. Write the progress note.
```

---

## Session 3 — Japan market module

```
[Prepend Master context]

GOAL: Show the Japan market (J-Quants free plan) and feed foreign-investor flows into the composite.

Tasks:
1. Sidecar adapter: jquants (FREE plan). Fetch OHLC, indices, investor-type trading flows (投資部門別), weekly margin balances (信用), short-selling ratio. ASSUME DELAYED DATA on the free plan and reflect it in freshness labels.
2. FX: USD/JPY. Futures (OSE/CME/SGX) to derive an overnight gap hint for the TSE open.
3. Store: jp_investor_flows, jp_margin, jp_short_selling, fx_rates.
4. IPC: get_jp_market(), get_watchlist()/set_watchlist().
5. Frontend "Japan Market" view: indices, sector heatmap, investor-type flows, margin/short, futures + gap hint, USD/JPY, watchlist.
6. Connect the foreign-investor-flow signal to the composite (weekly; hold last value; show staleness).

ACCEPTANCE:
- Japan view renders; foreign-flow contribution appears in the composite.
- The J-Quants free-plan delay is clearly surfaced to the user.

Write the progress note.
```

---

## Session 4 — Disclosure AI + Alerts

```
[Prepend Master context]

GOAL: Japanese AI summaries (cost-cascade) for disclosures/news, plus low-noise alerts.

Tasks:
1. Sidecar adapters: edinet (securities reports, large-holding reports, etc.), tdnet (timely disclosures — settle the access method), news (Japanese financial RSS/API), calendar (econ/earnings/FOMC/BOJ).
2. AI Router (§10): Tier1 rules (XBRL templated extraction) → Tier2 local Qwen MoE on RTX 5090 (free-text JP summarization) → Tier3 Claude API (daily brief). Cache results in DB, record summarized_tier, gate Tier3 with a daily budget.
3. Store: disclosures, news, calendar_events, alerts, alert_rules.
4. Alert Engine (§11): multi-source corroboration (≥N independent signal families), dedup, rate-limit, emit alert:fired.
5. IPC: get_disclosures(filter), get_alerts(), get_alert_rules().
6. Frontend: "Disclosures/News" view (with filters), "Alerts" view (history + rules); wire brief summary + latest alerts into Home.

ACCEPTANCE:
- Disclosures/news show Japanese summaries with the generating tier visible.
- Alerts fire only on multi-signal corroboration and persist to history.

Write the progress note.
```

---

## Session 5 — Cross-market + polish (v1 complete)

```
[Prepend Master context]

GOAL: Implement US→Japan transmission and finish settings/freshness/offline/errors for v1.

Tasks:
1. Compute: cross-market transmission rules (§9, editable). Write cross_market. IPC get_cross_market(). Frontend "Cross-Market" view ("observed premise → likely sector tilt", labeled as a hint, not a prediction).
2. "Settings" view: credentials via keychain (set_credential), update intervals, watchlist, composite weight tuning, layout.
3. Cross-cutting: polish freshness UI, offline cache rendering, error handling, logging (fetch failures, rate limits, tier usage).
4. Optional: ⌘K command palette.

ACCEPTANCE:
- Cross-market hints render.
- Settings can tune everything; credentials are stored in the keychain.
- Offline / fetch-failure degrades gracefully via cache.
- v1 scope (requirements §3.1) is satisfied.

Write the progress note and a short v1 release summary in docs/.
```
