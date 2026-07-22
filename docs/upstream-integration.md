# Upstream integration log

Fork parent (upstream): https://github.com/tw93/Pake — branch `main`.

Compared against merge-base `8b923d4` … `upstream/main` (`bf0c433`, 31 commits reviewed).

## Integrated

Cherry-picked (with `-x`) focused, low-risk fixes that apply cleanly and are
covered by the existing vitest suite or are self-contained:

| Upstream  | Fix                                                                    |
| --------- | ---------------------------------------------------------------------- |
| `9a63704` | fall back from malformed saved zoom (inject/event.js)                  |
| `ad088e3` | preserve native clipboard formats on paste (inject/event.js)           |
| `783135f` | guard clipboard paste fallback against key-repeat / stale arms         |
| `1a6cfa4` | stop duplicate new window when tray + multi-window combined (setup.rs) |

Also refreshed the stale `merge-window-options` snapshot to include the fork's
own `tabs` window field (pre-existing failure, unrelated to the picks).

## Reviewed but skipped

- `afc73ec` (CLI numeric-range enforcement), targets `bin/helpers/config-file.ts`
  and `schema/pake.schema.json`, which this browser-lite fork removed — not applicable.
- `1b97a50` (local-input staging) — conflicts with the fork's diverged `bin/cli.ts`
  and `bin/helpers/merge.ts`; not worth a risky manual merge for a packaging path.
- `83a1c47`, `5946b3e` (Windows icon re-assert / npm hotfixes) — good fixes, but
  conflict with the fork's customized `lib.rs`/`window.rs` setup flow and cannot be
  compile-verified here (no local Rust toolchain). Deferred rather than shipped blind.
- Feature/branding/docs commits (`--json` output, `--config` manifests, Notion app,
  `hide-window-decorations`, claude-code skill plugin, contributor/version bumps,
  doc-only changes) — out of scope; would add surface area against the fork's
  intentionally trimmed direction.
