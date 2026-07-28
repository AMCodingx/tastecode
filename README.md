# Personal Harness

> Working title. A beautiful, adaptive control panel for AI coding agents.
> Windows-first, cross-platform, and built around a design agent with real taste.

**Status: planning. No application code yet.** This repository currently holds research,
architecture decisions, and the build plan. See [`docs/`](./docs).

---

## What this is

Every serious coding agent today ships as a terminal program: Claude Code, Codex CLI,
Cursor CLI, OpenCode, Gemini CLI, Grok Build. They are powerful and ugly, and each one
locks you into its own session model, its own auth, its own config.

Personal Harness is the layer above them. One window. Every agent. Every subscription
you already pay for, plus your own API keys. Threads that load instantly at 500 messages.
Diffs you actually want to read. And a design agent that produces work you would ship,
not work that looks like it came out of a model.

We are not building another model wrapper. We are building the client the agents deserve.

## The three bets

1. **Aggregation.** Bring your own subscription *and* your own key — Claude, ChatGPT/Codex,
   Cursor, Kimi, GLM, Grok, OpenRouter, local models. The harness adapts to what you have.
2. **Craft.** The UI is the product. Speed, motion, typography, and density are features,
   not decoration. A 200-message thread must feel like a 5-message thread.
3. **Taste.** A first-class design agent (the *TasteSkill* system) that makes landing pages,
   portfolios, and product UI at a level ordinary agents cannot reach.

## Platforms

| Surface | Status |
| --- | --- |
| Windows desktop | Primary target, ships first |
| macOS desktop | Parity target, same codebase |
| Linux desktop | Best-effort |
| Mobile (remote control) | Planned after desktop v1 |
| Web / self-host | Falls out of the architecture |

## Team

Two people. One Windows developer, one macOS developer. Every decision in this repo assumes
both platforms are first-class and that no one has to leave their OS to review a change.

Read [`docs/05-team/working-rules.md`](./docs/05-team/working-rules.md) before your first commit.

## Repository map

```
docs/
  00-vision.md                  What we are building and why
  01-research/                  Field research: competitors, protocols, providers, UI perf
  02-decisions/                 ADRs — the decisions and what we rejected
  03-product/                   Product spec, design-agent spec, UX principles
  04-plan/                      Roadmap and milestones
  05-team/                      How we work: branches, commits, reviews
  06-landing/                   Landing page plan (pre-open-source)
  07-mobile/                    Mobile remote-control plan
```

## License

Undecided while private. See [`docs/02-decisions/ADR-0006-license-and-open-sourcing.md`](./docs/02-decisions/ADR-0006-license-and-open-sourcing.md).
The project is intended to be open sourced.
