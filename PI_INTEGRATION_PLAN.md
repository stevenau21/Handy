# Pi Agent Integration Plan (for stevenau21/Handy)

Repo: <https://github.com/earendil-works/pi> — Package: `@earendil-works/pi-coding-agent` v0.78.0 — License: MIT — Runtime: Node ≥ 22.19.0 OR Bun.

## Vision (revised after user clarification)

> "Pi does the heavy work — browsing, opening apps, agentic workflows. Handy is the voice I/O."

**Handy is the voice I/O, Pi is the executor.** The user speaks; Handy transcribes; if the request is a simple voice command (open URL, open app, send message), Handy handles it directly. If it's a complex request (browse a website, run a multi-step workflow, build a new Handy voice command on the fly), Handy routes it to Pi. Pi uses its `bash`, `read`, `write`, `edit`, `find`, `grep`, `ls` tools to do the work. Pi's text responses are pasted into the active app or shown in a side panel; tool executions are visible in the panel but not pasted.

This makes Handy → Pi a true **voice-driven agent harness**, with Pi's existing tool ecosystem (bash, file ops, and any user-supplied extensions) doing the "heavy work" Handy was never designed to do alone.

## What Pi already gives us for free

| User need | Pi's existing capability | How Handy calls it |
|---|---|---|
| Browse websites | `bash` tool can `curl`/`wget`/headless browser | `RpcClient.prompt` lets Pi decide to call bash |
| Open apps | `bash` tool can `start <app>` / `open -a <app>` | same |
| Agentic workflows | Pi's full agent loop with all 7 tools | `RpcClient.prompt` |
| Create new Handy voice commands | Pi's `read`/`write`/`edit` tools can edit `src-tauri/src/managers/commands.rs` or the settings JSON | same |
| File operations | Pi's `read`/`write`/`edit` | same |
| Search the web for info | bash + curl or any user-added tool | same |
| Multi-step reasoning | Pi's LLM loop, with `compact` for long sessions | automatic when using RpcClient |

The only thing Pi **doesn't** ship that the user might want is a dedicated headless-browser tool. Two ways to add it later:
- Use the `defineTool` Extension API to write a small `playwright` tool (TS, ~50 lines)
- Just use bash with `curl` / `lynx` for simple fetch

**We do not need to write any of this for v1.** Pi's bash tool already covers the user's stated needs.

## What does "Pi inside Handy" actually look like?

Pi runs **headless** (`pi --mode rpc`, no TUI). Handy provides a custom React chat-style panel that consumes Pi's streaming events:

```
┌─────────────────────────────────────────────────┐
│ Pi Agent                              ● Connected │
├─────────────────────────────────────────────────┤
│  You: hey pie, find me the best ramen in Bangkok │
│                                                 │
│  Pi: Searching...                               │
│  🔧 bash: curl -s "https://..."                 │
│  📄 result: ...                                 │
│  💬 Based on reviews, Mensho Tokyo in Sukhumvit │
│      is rated 4.7/5. Want me to book a table?   │
│                                                 │
│  You: yes, 7pm for 2                            │
│                                                 │
│  Pi: Opening booking site...                    │
│  🔧 bash: start https://...                    │
│  💬 Done. Booked at Mensho Tokyo, 7pm, 2 ppl.   │
│                                                 │
│  [🎤 hold to talk]  [Type a message...]  [Abort] │
└─────────────────────────────────────────────────┘
```

Pi's `bash` tool can spawn any CLI — including Claude Code, Codex, aider, etc. They'll run in a separate terminal window (since they are themselves TUIs), but the bash invocation is visible in the Pi panel. To make them first-class tools inside the panel (no separate window), wrap them as typed Pi tools via `defineTool` — small follow-up, see "Future extensions" below.

## TL;DR — what's the right build

**Tier B (in-process Pi panel using RpcClient) + Bundling B-iii (ship standalone `dist/pi` binary as Tauri resource) + voice routing that uses Pi as the executor when the user says a "pie" cue.**

Concretely:

1. **Add Pi as an npm dep** (`bun add @earendil-works/pi-coding-agent@0.78.0`).
2. **Build the standalone `pi` binary** once via `npm run build:binary` (needs Bun) and ship it as a Tauri resource.
3. **Add a `PiService` singleton in the React renderer** that wraps Pi's `RpcClient`. Lifecycle: lazy-start on first use, restart on crash, stop on app exit.
4. **Add a "Pi" tab to Handy's settings window** with: streaming chat history, mic button, text input, model selector, status indicator.
5. **Wire voice routing** with two cues:
   - `hey pie …` (or any custom phrase) → transcription routes to Pi instead of pasting
   - Everything else → current Handy behavior (paste or match existing voice command)
6. **Default `run_pi` voice command** in `VoiceCommandsSettings.tsx`'s defaults, with the user able to add their own.
7. **Auto-paste Pi's final text response** into the active app (so the user sees the answer in their context), with full chat history also visible in the Pi panel.
8. **i18n keys** for the new UI strings; ESLint-enforced (no hardcoded JSX text).
9. **No upstream PR** (feature freeze on cjpais/Handy).

## Why not just spawn `pi` in a terminal? (re-evaluating Tier A)

Tier A ("open pi in a terminal, type by hand or simulate keystrokes") is appealing because it's a 5-minute add, but the user's vision is **conversational** ("hey pie do this"). A terminal-only path means:

- You'd have to simulate keystrokes into a TUI, which is fragile and platform-specific.
- The user would have to *visually* watch the terminal — defeating the "voice-driven" goal.
- You can't get Pi's text back into the active app programmatically (you'd have to copy from the terminal).

Tier A is OK as a "version 0" to test the waters, but it doesn't deliver the user's vision. Skip it unless the user wants it.

## Why not embed the TUI in Handy? (re-evaluating Tier C)

Embedding Pi's full `InteractiveMode` (a TUI) inside a Tauri WebView is technically possible but extremely hard — TUIs need a real PTY, the TUI library uses ANSI escapes, and Handy's React/Tailwind UI is fundamentally different. Tier C's "pop out to terminal" button is fine (it's just an `open_app` call to Windows Terminal), but the in-app panel should use the typed RPC API, not the TUI.

## Final recommendation

**Tier B with B-iii, panel as a settings tab, voice cue is `hey pie` (configurable).**

Files to add/change (touched again with the revised scope):

| File | Change |
|---|---|
| `package.json` | Add `@earendil-works/pi-coding-agent@0.78.0` (pinned) |
| `src-tauri/capabilities/desktop.json` | Add `tauri-plugin-shell` `execute` scope for `pi` binary (and `node` for dev) |
| `src-tauri/src/managers/pi.rs` | **NEW** — `get_pi_binary_path`, `get_pi_cwd`, `get_pi_resource_dir` |
| `src-tauri/src/commands/pi.rs` | **NEW** — Tauri commands: `get_pi_binary_path`, `get_pi_cwd`, `check_pi_auth` |
| `src-tauri/src/managers/commands.rs` | Add `"run_pi"` arm that emits a `pi-prompt` event |
| `src-tauri/src/actions.rs` | Voice cue detection: if transcription matches a "pie" cue, route to Pi instead of pasting |
| `src/components/pi/PiPanel.tsx` | **NEW** — chat UI: history, mic, text input, streaming |
| `src/components/pi/PiMessage.tsx` | **NEW** — single message bubble (user/assistant/tool) |
| `src/components/pi/piService.ts` | **NEW** — singleton wrapping `RpcClient`, manages lifecycle, exposes `prompt`, `abort`, `onEvent` |
| `src/components/pi/extensions/` | **NEW** — folder for custom `defineTool` wrappers (e.g. `claude-code.ts` to launch Claude Code as a typed tool inside the Pi panel, future `playwright.ts`) |
| `src/hooks/usePiService.ts` | **NEW** — React hook around the singleton |
| `src/components/settings/Settings.tsx` | Add "Pi Agent" tab to settings |
| `src/components/settings/commands/VoiceCommandsSettings.tsx` | Add a default `hey pie` voice command |
| `src/i18n/locales/en/translation.json` | New `pi.*` keys (see PI_INTEGRATION_PLAN.md history) |
| `src-tauri/tauri.conf.json` | Add `bundle.resources` entry for the `dist/pi` binary |
| `GUIDE_Adding_Voice_Commands.md` | Add a "Pi voice command" section |
| `scripts/build-pi-binary.{sh,ps1}` | **NEW** — script to run `npm run build:binary` from a pinned Pi checkout and copy the artifact into `src-tauri/resources/` |

## Future extensions (post-v1)

- **Custom `defineTool` wrappers** in `src/components/pi/extensions/`:
  - `claude-code.ts` — spawns `claude` as a typed tool, results stream into the Pi panel
  - `playwright.ts` — headless browser for sites that need JS
  - `github.ts` — typed wrapper around `gh` CLI (PRs, issues, releases)
- **Pi's `extension_ui_request` → Handy dialogs** — Pi can ask "select from this list" or "confirm yes/no" or "input text" via its RPC `extension_ui_request` event. Wire those to a Handy dialog component so Pi's prompts feel native.
- **Pi session persistence** — keep the same conversation across Handy restarts (Pi already supports this via its `SessionManager`).
- **Multi-cue routing** — different phrases trigger different Pi behaviors (e.g. "hey pie write" → file-edit mode, "hey pie browse" → bash + curl only).
- **Voice loop** — pipe Pi's text response through TTS (Handy would need a TTS provider added; today it has none).

## Implementation order (revised)

1. `bun add @earendil-works/pi-coding-agent@0.78.0`
2. `src/components/pi/piService.ts` — RpcClient wrapper
3. `src/components/pi/PiPanel.tsx` — basic chat UI, no voice
4. Add tab to settings
5. i18n keys (en) + ESLint passes
6. Add `run_pi` voice command (Tier B1 path)
7. Voice cue routing in `actions.rs` (Tier B2 path: any transcription matching "hey pie …" → Pi)
8. Wire mic button in PiPanel
9. Auto-paste Pi's final text response
10. `scripts/build-pi-binary.ps1` to produce `dist/pi` from a pinned checkout
11. `src-tauri/src/managers/pi.rs` and Tauri command
12. `src-tauri/tauri.conf.json` resource bundle
13. `bun run lint && bun run format && bun run format:backend`
14. Conventional commit (`feat:`)
15. NO upstream PR (feature freeze)

## Pre-flight checklist before any code

- [x] Tier chosen: **B (in-process Pi panel)**
- [ ] Bundling chosen: **B-iii (ship standalone `dist/pi` binary)** — needs Bun on the dev box for the build step; user has Bun per AGENTS
- [ ] User confirms: Bun on dev box + at least one Pi-supported model provider authed (Anthropic / OpenAI / Google)
- [ ] User confirms: voice cue phrasing (`hey pie` / `ask pie` / configurable)
- [ ] User confirms: auto-paste Pi's text response (yes / no / configurable)
- [ ] User confirms: window model (settings tab / separate Tauri window)