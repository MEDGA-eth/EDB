# EDB Web UI — design spec

- **Date:** 2026-05-08
- **Status:** Approved (brainstorming output, ready for implementation plan)
- **Author:** Zhuo Zhang (with Claude in brainstorming mode)
- **Tracking:** `docs/superpowers/specs/2026-05-08-edb-web-ui-design.md`

## 1. Summary

Add a browser-based UI for EDB that talks directly to the engine's JSON-RPC API, opt-in via `edb replay --ui=web <tx>`. The UI is implemented as a React/TypeScript SPA built with bun, embedded into the `edb` binary at compile time, and served by the engine's existing Axum server on the same port as the JSON-RPC endpoint. The TUI remains the default and is unchanged.

The visual identity follows the [tiny-dec website](https://github.com/ZhangZhuoSJTU/tiny-dec/tree/main/website): warm cream / candy-pastel light theme by default, with a dark-mode toggle shipped on day one. Both themes share rounded corners, soft shadows, and the Quicksand / Nunito / Fira Code font stack.

v1 ships with full TUI parity (all four panels, breakpoints, expression evaluation) plus web-native moves the TUI can't easily do (resizable panels, mobile/tablet stacking, URL-shareable snapshot state).

## 2. Goals

- `edb replay --ui=web <tx>` starts the engine, opens a browser, and presents the full debugger interface
- Single-port deployment: no extra binaries, no CORS, no second listener
- Self-contained `edb` binary in production (frontend assets embedded; no separate web server to ship)
- Feature parity with the TUI for snapshot navigation, source/opcode inspection, variable/stack/memory/storage display, breakpoints, and expression evaluation
- Distinctive, polished visual identity matching tiny-dec; dark mode included
- Comprehensive test coverage at component, hook, store, integration, and E2E layers

## 3. Non-goals

- Replacing the TUI in v1 (TUI stays the default; web is opt-in)
- Multi-user / hosted / authenticated deployments (localhost only)
- Real-time push from engine to browser (engine RPC is read-only and stateless; pull is sufficient)
- Multi-session UI (one transaction at a time, like the TUI)
- Reusing the TUI's `data/manager` layer in the browser (the JS query stack handles caching)
- Codegen of TS types from Rust types (manual `lib/types.ts` for v1; revisit if maintenance hurts)
- Visual snapshot regression tests (Percy/Chromatic) — Playwright failure screenshots are enough for v1

## 4. Architecture

### 4.1 Workspace layout

A new workspace member crate `crates/web/`:

```
crates/web/
├── Cargo.toml
├── build.rs                    # runs `bun install --frozen-lockfile && bun run build`
├── src/
│   └── lib.rs                  # pub fn router() -> axum::Router
└── frontend/
    ├── package.json
    ├── bun.lock
    ├── tsconfig.json
    ├── vite.config.ts          # bun reads vite configs; defines dev proxy
    ├── index.html
    ├── src/
    │   ├── main.tsx
    │   ├── App.tsx
    │   ├── components/
    │   │   ├── panels/         # CodePanel, TracePanel, DisplayPanel, TerminalPanel
    │   │   ├── TopBar.tsx
    │   │   ├── HelpOverlay.tsx
    │   │   ├── ThemeToggle.tsx
    │   │   ├── ConnectionIndicator.tsx
    │   │   ├── ErrorCard.tsx
    │   │   └── SessionEndedOverlay.tsx
    │   ├── hooks/              # one per RPC method (useSnapshotInfo, useTrace, ...)
    │   ├── store/session.ts    # Zustand: currentSnapshotId, breakpoints, terminalHistory, theme
    │   ├── lib/
    │   │   ├── rpc.ts          # JSON-RPC client (fetch + zod validation)
    │   │   ├── types.ts        # TS mirrors of engine RPC types
    │   │   └── theme.ts        # token tables (light + dark)
    │   ├── styles/             # tailwind v4 entry + theme tokens
    │   └── data/mocks/         # canned RPC fixtures for tests + bun-dev mock mode
    ├── e2e/                    # Playwright tests
    └── dist/                   # gitignored; build output, embedded by Rust
```

`Cargo.toml` updates:
- Workspace `members` list gains `crates/web/`
- Workspace dependencies gain `edb-web = { version = "0.0.2", path = "crates/web" }`
- `crates/edb/Cargo.toml` gains `edb-web.workspace = true`

### 4.2 Single-port routing

Axum's `Router::merge` composes the existing engine RPC routes with the new static-file routes. Routes are method-disjoint at the root path:

| Method | Path | Handler |
|---|---|---|
| POST | `/` | engine JSON-RPC (existing) |
| GET | `/health` | engine health (existing) |
| GET | `/*path` | `edb_web` static-file handler with SPA fallback to `index.html` (new) |

This means the existing TUI client (`edb-tui`) continues to work unchanged — it POSTs to `/`, hits the JSON-RPC handler, never touches the GET routes.

### 4.3 Engine integration

Add one method to `DebugRpcServer`:

```rust
// crates/engine/src/rpc/server.rs
impl<DB> DebugRpcServer<DB> {
    pub fn with_extra_router(mut self, extra: Router) -> Self {
        self.extra_router = Some(extra);
        self
    }
}
```

Inside `start_on_port`, the final app is built as `app.merge(extra_router.unwrap_or_default())`. The engine takes a generic `axum::Router`, not `edb_web` specifically, so the engine remains UI-agnostic.

### 4.4 CLI wiring

`crates/edb/src/main.rs`:
- `Cli` gains `#[arg(long, value_enum, default_value = "tui")] ui: Ui` where `enum Ui { Tui, Web }`

`crates/edb/src/cmd/replay.rs`:
- After `engine.prepare(...)`, branch on `cli.ui`:
  - `Ui::Tui` → existing flow (spawn `edb-tui` binary)
  - `Ui::Web` → mount `edb_web::router()` via `with_extra_router` before `start_on_port`; once the listener is up, call `webbrowser::open(format!("http://127.0.0.1:{port}/"))?` and block on Ctrl+C

The TUI launch path is unchanged when `--ui=tui` (the default).

### 4.5 Build pipeline (Approach A)

`crates/web/build.rs`:
- `cargo:rerun-if-changed=frontend/src` (and `frontend/index.html`, `frontend/package.json`, `frontend/bun.lock`, `frontend/tsconfig.json`, `frontend/vite.config.ts`)
- Verifies `bun --version` succeeds; on failure, panics with: "bun not found — install via `curl -fsSL https://bun.sh/install \| bash` or set `EDB_SKIP_WEB_BUILD=1` to skip"
- Runs `bun install --frozen-lockfile` then `bun run build` from `frontend/`
- `EDB_SKIP_WEB_BUILD=1` short-circuits build.rs (used in editor/IDE environments where bun isn't available; the resulting binary will fail at startup if web routes are needed, but cargo check stays fast)

`frontend/dist/` is consumed by `rust_embed::Embed` derive in `lib.rs`. `lib.rs` exposes:

```rust
pub fn router() -> axum::Router { /* serves embedded assets, SPA fallback */ }
```

### 4.6 Dev workflow

Two terminals:
1. `edb replay <tx>` — engine starts on its assigned port
2. `cd crates/web/frontend && bun dev` — bun's dev server starts on `:5173` with HMR

`vite.config.ts` proxies POST `/` and GET `/health` to `http://localhost:${VITE_EDB_RPC_URL_PORT:-8545}`. All other paths are bun-served. URL paths match production exactly.

A `bun dev --mock` mode (env flag in vite config) replaces the proxy with a fixture-based RPC handler reading `frontend/src/data/mocks/` — useful for working on UI without running an engine.

### 4.7 CI integration

- Add `oven-sh/setup-bun@v2` to existing CI jobs that compile `edb-web` (specifically the workspace `cargo build` / `cargo check` / `cargo test` / `cargo clippy` steps).
- New `web-frontend` job (Linux only): installs bun, runs `bun install --frozen-lockfile && bun test --coverage` in `crates/web/frontend/`.
- New `web-e2e` workflow (PR-only, slow): builds `edb` release binary, installs Playwright, runs `bun run e2e` against a fixture transaction. Runs in its own workflow file so it doesn't block the regular CI matrix.

## 5. Components & state

### 5.1 Layout

Desktop (≥1280px): 4-pane grid with draggable splitters via `react-resizable-panels`.

```
┌──────────────────────────────────────────────────────────┐
│ TopBar: tx hash · breadcrumbs · theme toggle · help (?)  │
├────────────────────────────┬─────────────────────────────┤
│  CodePanel                 │  TracePanel                 │
├────────────────────────────┼─────────────────────────────┤
│  DisplayPanel              │  TerminalPanel              │
└────────────────────────────┴─────────────────────────────┘
```

- `1024–1279px`: panels collapse into a tab bar (Code / Trace / Display / Terminal).
- `<1024px`: single-panel mobile layout with a swipe/tap switcher.
- TopBar persists across all breakpoints.

### 5.2 Panel responsibilities

| Panel | Responsibility | Key components |
|---|---|---|
| **CodePanel** | Solidity source (hook snapshots) or disassembled opcodes (opcode snapshots). Highlights current PC/line. Clickable gutter for breakpoints. | CodeMirror 6 + Solidity language pack; custom `<pre>` tokenizer for opcodes (TS port of `crates/tui/src/ui/syntax/`) |
| **TracePanel** | Nested call tree; opcode/hook badges; clickable to jump to a frame's first snapshot. Expand/collapse. | Custom tree component (or `react-arborist` if it fits the aesthetic) |
| **DisplayPanel** | 5 sub-tabs: Variables, Stack, Memory, Storage (with diff highlight), Transient. | Tab strip + per-tab views; storage diff renders added/removed/modified using tiny-dec's color tokens |
| **TerminalPanel** | REPL: input, send, history. Commands map 1:1 with engine RPC (eval, breakpoint, navigation). Output rendered with `react-markdown` for rich formatting. | Input row + scrollback; history persists to localStorage |
| **TopBar** | Tx hash (link to Etherscan), snapshot id / total, prev/next nav buttons, dark-mode toggle, help button, connection indicator | Combinator of small components |
| **HelpOverlay** | Full-screen modal: keybindings + command reference | Modal + markdown rendering |

### 5.3 State management

- **TanStack Query** for engine-derived state. All queries use `staleTime: Infinity` since engine data is immutable per session. queryKey is `[method, ...params]` — e.g. `['snapshot', id]`, `['code', id]`, `['storage', id, slot]`.
- **Zustand** store (`store/session.ts`) for UI/session state: `currentSnapshotId`, `breakpoints[]`, `terminalHistory[]`, `panelTab`, `theme`. `theme` and `terminalHistory` persist to localStorage; the rest are session-only.
- **URL hash** mirrors `currentSnapshotId`. On mount, hash → store. On snapshot change, store → hash. Refresh preserves position; URLs are shareable within a debugging session.
- **No global router** (no `react-router`). HelpOverlay and SessionEndedOverlay are rendered conditionally based on local state.

### 5.4 Theme system

- Tailwind v4 `@theme` directive declares token names: `--bg`, `--bg-elevated`, `--fg`, `--fg-secondary`, `--accent`, `--phase-fe/an/be`, syntax tokens, font families, `--radius`, shadows.
- Light theme uses tiny-dec's `index.css` palette verbatim (warm cream + pastels).
- Dark theme is a parallel set of tokens defined under `[data-theme="dark"]`. Designed alongside the light theme — not retrofitted.
- `<html data-theme="light|dark">` controls active theme; ThemeToggle flips it and writes to localStorage. Initial value: localStorage if set, else `prefers-color-scheme`.

### 5.5 Frontend dependencies

| Package | Purpose |
|---|---|
| `react`, `react-dom` (^19) | UI |
| `@tanstack/react-query` (^5) | server-state cache |
| `zustand` (^5) | UI/session state |
| `react-resizable-panels` | splitters |
| `codemirror`, `@codemirror/lang-*`, `@codemirror/theme-one-dark` (or custom) | code editor |
| `react-markdown` | terminal output, help overlay |
| `tailwindcss`, `@tailwindcss/vite` (^4) | styling |
| `zod` | runtime validation of RPC responses |
| `typescript` | types |

Dev-only:
| Package | Purpose |
|---|---|
| `@testing-library/react`, `@testing-library/user-event` | component tests |
| `happy-dom` | DOM env for `bun test` |
| `playwright` | E2E |
| `@vitejs/plugin-react` (read by bun) | React in vite config |

## 6. Data flow

### 6.1 Boot

```
edb replay --ui=web 0xabc...
  ├─ fork_and_prepare()
  ├─ Engine::prepare()              → engine state ready
  ├─ DebugRpcServer::new(ctx)
  │     .with_extra_router(edb_web::router())
  │     .start_on_port(port)        → Axum binds 127.0.0.1:port
  ├─ webbrowser::open("http://127.0.0.1:port/")
  └─ block on Ctrl+C
```

Browser loads the embedded SPA; React mounts; `<App>` reads URL hash for `currentSnapshotId` (default 0); TanStack Query fires `edb_getSnapshotCount` and `edb_getTrace` in parallel.

### 6.2 RPC method ↔ hook mapping

| Hook | RPC method | queryKey |
|---|---|---|
| `useSnapshotCount()` | edb_getSnapshotCount | `['count']` |
| `useSnapshotInfo(id)` | edb_getSnapshotInfo | `['snapshot', id]` |
| `useTrace()` | edb_getTrace | `['trace']` |
| `useCode(id)` | edb_getCode | `['code', id]` |
| `useCodeByAddress(addr)` | edb_getCodeByAddress | `['code-addr', addr]` |
| `useContractABI(addr)` | edb_getContractABI | `['abi', addr]` |
| `useCallableABI(addr)` | edb_getCallableABI | `['callable', addr]` |
| `useConstructorArgs(addr)` | edb_getConstructorArgs | `['ctor', addr]` |
| `useStorage(id, slot)` | edb_getStorage | `['storage', id, slot]` |
| `useStorageDiff(id)` | edb_getStorageDiff | `['storage-diff', id]` |
| `useNextCall(id)` | edb_getNextCall | `['next-call', id]` |
| `usePrevCall(id)` | edb_getPrevCall | `['prev-call', id]` |
| `useBreakpointHits(bp)` | edb_getBreakpointHits | `['bp-hits', stable-hash(bp)]` |
| `useEvalExpr` (mutation) | edb_evalOnSnapshot | result cached at `['eval', id, expr]` after success |

### 6.3 RPC client

`lib/rpc.ts` is a plain `fetch` wrapper:

```ts
async function rpc<T>(method: string, params?: unknown[]): Promise<T> {
  const res = await fetch('/', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', method, params, id: nextId() }),
  });
  const json = await res.json();
  if (json.error) throw new RpcError(json.error.code, json.error.message);
  return json.result;
}
```

Each hook calls `rpc('edb_<method>', args)` and validates the response with a zod schema specific to that method. Validation failure throws `SchemaError` with the offending shape attached.

### 6.4 Navigation

User presses Next → `setCurrentSnapshotId(id+1)` in Zustand → URL hash syncs → panels re-render. Each panel's hook checks the cache; hit = instant render, miss = skeleton + fetch. Cache hits are common because navigation often revisits prior snapshots.

### 6.5 Expression eval

```
user types `bal(alice)` ↵ in TerminalPanel
  ↓
useEvalExpr.mutateAsync({ id: currentId, expr })
  ↓ POST / { method: edb_evalOnSnapshot, params: [id, expr] }
on success → append { kind: 'result', expr, value } to terminalHistory; cache at ['eval', id, expr]
on error   → append { kind: 'error', expr, code, message } to terminalHistory
```

`terminalHistory` persists to localStorage so reload preserves the conversation.

### 6.6 Breakpoints

Breakpoints live in the Zustand store (per-session, not persisted — they're tied to a specific transaction). Adding one fires `useBreakpointHits` which returns matching snapshot IDs from `edb_getBreakpointHits`. CodePanel renders a dot in the gutter at the breakpoint line; a Breakpoints sub-tab in DisplayPanel lists hits as clickable links that navigate.

## 7. Error handling

| Failure | Where | Handling |
|---|---|---|
| `bun` missing on `$PATH` at build time | `build.rs` | Panic with install instructions; `EDB_SKIP_WEB_BUILD=1` to bypass |
| `bun build` fails | `build.rs` | Surface bun's stderr; panic prefixed `[edb-web build]` |
| `frontend/dist/` empty when serving (e.g. built with `EDB_SKIP_WEB_BUILD=1`) | `router()` startup | If the embedded asset count is 0, `router()` panics with: "edb-web has no embedded assets — rebuild without `EDB_SKIP_WEB_BUILD=1` and ensure `bun` is on `$PATH`" |
| Engine RPC error code (e.g. `SNAPSHOT_OUT_OF_BOUNDS`) | per-panel hook | TanStack Query `onError` → `<ErrorCard code={...} message={...} />` with Retry |
| Transport error (fetch throws) | `lib/rpc.ts` | Wrap in `TransportError`; ConnectionIndicator yellow on 1 miss, red on 3 |
| Engine dead | `useHealthcheck()` (poll every 2s) | After 3 misses, `<SessionEndedOverlay>` blocks all interactions |
| Render error (single panel) | per-panel `<ErrorBoundary>` | Panel falls back; others keep working |
| Render error (top-level) | `<AppErrorBoundary>` in `main.tsx` | Full-page fallback with reload button |
| Browser open fails | `edb` CLI | Catch from `webbrowser::open`; print URL prominently and continue |
| Dev: bun running before engine | `useHealthcheck()` | Same overlay; auto-recovers when first health check succeeds |
| Schema mismatch (API drift) | `lib/rpc.ts` zod | `SchemaError` shown in panel, with offending shape logged |

ConnectionIndicator on the TopBar: 🟢 connected / 🟡 degraded (1 miss or slow) / 🔴 offline (3+ misses or `/health` non-200).

Backward compat: `with_extra_router` doesn't touch existing routes, so `edb-tui` is unaffected.

## 8. Testing

### 8.1 Frontend (`bun test` + `happy-dom` + `@testing-library/react`)

1. **Component tests** — every panel, every shared component (TopBar, ThemeToggle, ConnectionIndicator, ErrorCard, SessionEndedOverlay, HelpOverlay). Mock RPC layer per test. Coverage target: ≥80% statements per component file.
2. **Hook tests** — every `useXxx` hook. Use `renderHook` with a fresh `QueryClient`. Cases: success, engine error code, transport error, schema mismatch, cache hit doesn't refetch.
3. **Store tests** — Zustand actions (next/prev snapshot, addBreakpoint, clearHistory, theme toggle) as pure unit tests.
4. **`lib/rpc.ts` tests** — mocked `fetch`; verify request shape, error decoding, transport-error wrapping, zod validation success + failure paths.
5. **Theme tests** — toggle behavior, localStorage persistence, `prefers-color-scheme` initial selection, `data-theme` attribute correctness.
6. **Mock-RPC fixtures** — `frontend/src/data/mocks/` checked-in JSON for one sample transaction (e.g., a small Uniswap swap). Used by component tests and by `bun dev --mock`. Regenerated by a `bun run mocks:gen` script that hits a real engine.

### 8.2 Backend (`crates/web/tests/`)

- `static_files.rs` — builds an Axum test app from `edb_web::router()`, asserts:
  - `GET /` returns 200 + `text/html` + contains `<div id="root">`
  - SPA fallback: `GET /foo/bar` returns the same `index.html`
  - Asset paths return correct content-types and non-empty bodies
- `merge_with_engine.rs` — mounts a stub engine RPC router + `edb_web::router()`, verifies:
  - `POST /` reaches the JSON-RPC handler (route precedence not stolen)
  - `GET /index.html` reaches the static handler
  - `GET /health` reaches the engine health probe

### 8.3 End-to-end (Playwright in `crates/web/frontend/e2e/`)

Three flows for v1, all against a real `edb replay --ui=web <fixture-tx>`:
1. Happy-path navigation — load UI, all 4 panels render, click Next 5×, URL hash and panels both update.
2. Expression eval — type `block.number` in terminal, result appears in history.
3. Breakpoint set + jump — click a gutter line, dot appears, click hit in Breakpoints view, navigation works.

Fixture transaction: one of the existing `crates/integration-tests/` known transactions.

### 8.4 CI integration

- Existing `cargo test --workspace --all-features` automatically picks up `crates/web/tests/*.rs`.
- New `web-frontend` job (Linux only): `setup-bun@v2` → `bun install --frozen-lockfile && bun test --coverage`.
- New `web-e2e` workflow file (PR-triggered, slow): builds release `edb` binary, installs Playwright, runs the 3 E2E flows. Runs separately from the main CI matrix so it doesn't block other jobs.

## 9. Open questions / deferred decisions

These are explicitly deferred, not unresolved:

- **TS type codegen from Rust.** Manual `lib/types.ts` for v1. Revisit if drift becomes a maintenance pain — `ts-rs` or hand-rolled `serde-typescript` are options.
- **CodeMirror language pack for Solidity.** v1 uses a community pack (e.g., `@replit/codemirror-lang-solidity`); if quality is poor, write a thin custom one or fall back to plaintext + token classes.
- **Visual regression testing.** Out of scope; revisit if UI churn breaks things silently.
- **Multi-session UI.** Single tx at a time in v1, mirroring the TUI.
- **Auto-reconnect after engine restart.** Not in v1; the SessionEndedOverlay is terminal. Possible v2 if the `server` command's WebSocket session model is integrated.

## 10. Out of scope

- Authentication / multi-user / hosted deployment
- WebSocket push from engine (engine RPC is read-only and stateless)
- Reusing the TUI's `data/manager` Rust layer in the browser
- Replacing the TUI in v1
- Codegen of TS types
- Visual snapshot regression tests
