# Fork Model Routing Progress

## Goal

This fork is moving toward a "single provider, multiple models" workflow where:

- the main thread model stays fixed
- cost optimization happens through delegated subagents
- custom agents can pin a model
- skills can route spawned subagents to a better-fit model using a user-owned model profile file

This note summarizes what has already been implemented so far.

## Implemented Changes

### 1. Fork user config home

Default user config home was changed from `~/.codex` to `~/.codex-fork`.

Current behavior:

- if `CODEX_HOME` is set, Codex still uses it
- if `CODEX_HOME` is unset, this fork now defaults to `~/.codex-fork`

Files touched:

- `codex-rs/utils/home-dir/src/lib.rs`
- docs/comments updated in:
  - `codex-rs/core/src/config/mod.rs`
  - `codex-rs/cli/src/mcp_cmd.rs`

### 2. Custom agent model support

Custom agent role files continue to use the existing TOML format.

Supported sources:

- user-level: `~/.codex-fork/agents/*.toml`
- project-level: `<repo>/.codex/agents/*.toml`

These files can now be used in practice to set:

- `model`
- `model_reasoning_effort`

Spawn precedence currently behaves as:

1. explicit `spawn_agent(model=...)`
2. custom agent role file defaults
3. inherited parent model

This means explicit spawn-time model overrides now beat role defaults.

Important unchanged behavior:

- built-in agents were intentionally left alone
- full-history fork still rejects child model overrides

Files touched:

- `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- tests updated in `codex-rs/core/tests/suite/subagent_notifications.rs`

### 3. Skill routing metadata

Skill metadata now supports routing tags in `agents/openai.yaml`.

New shape:

```yaml
routing:
  task_tags:
    - research
    - deep-reasoning
```

Currently accepted tags:

- `research`
- `coding`
- `review`
- `classification`
- `fast-cheap`
- `deep-reasoning`

Routing metadata is optional. If omitted, old behavior is unchanged.

Core skill model changes:

- `SkillMetadata` now has optional `routing`
- `SkillRouting` contains `task_tags`
- loader validates and normalizes these tags

Files touched:

- `codex-rs/core-skills/src/model.rs`
- `codex-rs/core-skills/src/loader.rs`

### 4. App-server / protocol / TUI propagation for skill routing

The new skill routing metadata was carried through the protocol and UI mapping layers so the field is not core-only.

Files touched:

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/plugin.rs`
- `codex-rs/app-server/src/request_processors/catalog_processor.rs`
- `codex-rs/tui/src/chatwidget/skills.rs`

Some test fixtures and in-memory test skills were updated to include `routing: None`.

### 5. User-level model profiles file

A new user-owned TOML file was introduced:

- `~/.codex-fork/model-profiles.toml`

Its purpose is to describe model strengths and tags for subagent routing.

Current implemented shape supports entries like:

- `model`
- `task_tags`
- `strengths`
- `cost_tier`
- `speed_tier`
- `notes`

This is implemented in:

- `codex-rs/core/src/subagent_model_routing.rs`

That module currently contains:

- profile file path resolution
- TOML load/parse
- skill-tag-to-model matching
- fallback reporting
- a simple profile template generator

### 6. Skill-based subagent routing

When a `spawn_agent` request contains explicit skill input items and no explicit model override is provided:

1. the mentioned skills are collected
2. their routing tags are read
3. `model-profiles.toml` is loaded from `codex_home`
4. available models are loaded from the current provider
5. the best matching profile is selected
6. the selected model is applied to the child config before explicit user overrides

If routing fails, the child safely falls back to the parent model.

Current fallback reasons include cases like:

- model profiles file missing
- no matching profile tags
- no matching available model

This behavior is implemented in:

- `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`

### 7. Spawn observability

Subagent spawn begin/end events now include:

- `route_reason`
- `fallback_reason`

This was added so you can inspect why a child model was chosen or why routing fell back.

Files touched:

- `codex-rs/protocol/src/protocol.rs`
- corresponding construction sites in spawn handlers
- one protocol test fixture in:
  - `codex-rs/app-server-protocol/src/protocol/thread_history.rs`

### 8. CLI helper for writing model profiles

Added:

```bash
cargo run -p codex-cli -- debug models --write-profile
```

Current behavior:

- reads currently available models
- generates a local TOML profile template
- writes it to `~/.codex-fork/model-profiles.toml`

Important limitation:

- this command currently uses local heuristic rules
- it does **not** ask the configured model to intelligently evaluate models yet

Files touched:

- `codex-rs/cli/src/main.rs`
- `codex-rs/cli/tests/debug_models.rs`

## Local Provider Config Added

Current local fork config created during setup:

Path:

- `~/.codex-fork/config.toml`

Current contents:

```toml
model = "cx/gpt-5.4"
review_model = "kr/claude-sonnet-4.5"
model_provider = "local-provider"

[model_providers.local-provider]
name = "Local Multi-Model Provider"
base_url = "http://localhost:20128/v1"
experimental_bearer_token = "sk-ea35bb61746ba7a5-rr1xy2-52287066"
wire_api = "responses"
requires_openai_auth = false
```

## Important Current Limitation

The fork currently does **not fully support raw provider model names end-to-end**.

Observed behavior:

- the local provider `/v1/models` returns namespaced IDs such as:
  - `cx/gpt-5.4`
  - `kr/claude-sonnet-4.5`
- but `cargo run -p codex-cli -- debug models` still shows canonicalized/internal names like:
  - `gpt-5.5`
  - `gpt-5.4`
  - `gpt-5.4-mini`
  - `gpt-5.3-codex`
  - `gpt-5.2`

What this means:

- some internal model metadata lookup already has partial namespaced support
- but `list_models`, `debug models`, spawn-time validation, and profile generation are not yet consistently preserving original provider model IDs

So the current implementation is enough to exercise the new routing framework, but not yet enough to make raw provider model names the single canonical model identity everywhere.

## Verified So Far

These checks were run successfully during implementation:

- `cargo test -p codex-core-skills`
- `cargo test -p codex-utils-home-dir`
- `cargo test -p codex-core spawn_agent_requested_model_and_reasoning_override_role_defaults`
- `cargo test -p codex-core spawn_agent_routes_skill_tagged_subagent_to_profile_model`
- `cargo test -p codex-core subagent_model_routing::tests::resolve_skill_routed_model_uses_best_matching_profile -- --exact`
- `cargo test -p codex-core subagent_model_routing::tests::resolve_skill_routed_model_reports_fallback_when_no_profile_matches -- --exact`
- `cargo test -p codex-cli --test debug_models`
- `cargo test -p codex-app-server --no-run`
- `cargo test -p codex-cli --no-run`
- `cargo fmt --all`

Notes:

- `just fmt` was not available locally, so `cargo fmt --all` was used
- larger rebuilds briefly hit local disk exhaustion because `codex-rs/target` had grown to ~46G, then `cargo clean` was used

### 9. Namespace-aware model name matching in spawn validation

`find_spawn_agent_model_name` (in `multi_agents_common.rs`) was updated to handle the
two coexisting naming conventions — bundled canonicalized names (e.g. `gpt-5.4`)
and provider-namespaced names (e.g. `cx/gpt-5.4`).

New matching order:

1. exact match first
2. if the requested name has a namespace prefix (`cx/gpt-5.4`), try matching the
   bare suffix (`gpt-5.4`) against the available set
3. if the requested name has no namespace (`gpt-5.4`), try matching against any
   namespaced entry whose suffix matches (`cx/gpt-5.4`)

This means a profile can use either naming convention and spawn validation will
resolve the model correctly, preferring the namespaced name when available so
the provider receives an identifier it recognizes.

The profile itself (`model-profiles.toml`) is now a fully user-owned artifact.
The recommended practice is to use namespaced IDs (e.g. `cx/gpt-5.4`,
`kr/claude-sonnet-4.5`) in the profile since those match what the provider
returns and are the canonical form at the API layer.

Files touched:

- `codex-rs/core/src/tools/handlers/multi_agents_common.rs`

## Tag Reference for Manual Profile Authoring

Task tags are free-form strings — there is no enum validation. These are the
six tags used by the heuristic generator and known to work well:

| Tag | Used for | Suggested model characteristics |
|-----|----------|--------------------------------|
| `research` | Information gathering, browsing, digging | Stronger, more expensive |
| `coding` | General code generation and editing | Default mid-range model |
| `review` | Code review, diff analysis | Good at spotting issues |
| `classification` | Simple categorization | Cheapest available |
| `fast-cheap` | Lightweight delegation, high throughput | Fastest/cheapest model |
| `deep-reasoning` | Complex reasoning, planning | Highest reasoning capability |

To activate routing for a skill, add a `routing` block to its
`agents/openai.yaml`:

```yaml
routing:
  task_tags:
    - research
    - deep-reasoning
```

The model profile file lives at `~/.codex-fork/model-profiles.toml`. Example:

```toml
version = 1

[[models]]
model = "cx/gpt-5.5"
task_tags = ["research", "deep-reasoning", "review"]
strengths = ["Best for deep reasoning and research tasks"]
cost_tier = "high"
speed_tier = "slow"

[[models]]
model = "cx/gpt-5.4"
task_tags = ["coding", "review"]
strengths = ["General-purpose coding and review"]
cost_tier = "medium"
speed_tier = "medium"

[[models]]
model = "cx/gpt-5.4-mini"
task_tags = ["fast-cheap", "classification"]
strengths = ["Low-cost for lightweight delegation"]
cost_tier = "low"
speed_tier = "fast"
```

## Next Steps (deferred)

- AI-powered profile evaluation: decided against implementing. The
  `model-profiles.toml` is a user-authored artifact that's edited infrequently.
  The heuristic `build_suggested_model_profiles` + manual editing is sufficient.

---

## Debug Diary: Subagent Model Routing

### Problem: Skill-based routing never selects a model

When `@test-routing` was invoked and the model called `spawn_agent`, the
subagent always inherited the parent model instead of being routed to a
profile-matched model.

### Root causes (chronological)

#### 1. Provider models never loaded

`should_refresh_models()` only returned true for Codex backend or command-auth
providers. Local providers with bearer-token auth were excluded, so
`/v1/models` was never called. Only the 6 bundled `models.json` entries were
available — namespaced models like `kr/claude-sonnet-4.5` or
`ld/claude-opus-4-7[1m]` were absent.

**Fix** (`models-manager/src/manager.rs`, `model-provider/src/models_endpoint.rs`):
Added `has_configured_endpoint()` to the trait and `should_refresh_models`
now also checks it — any provider with a non-default `base_url` gets its
model list fetched.

#### 2. OpenAI `/v1/models` format not parsed

The local provider returns standard OpenAI format `{data: [{id: "...", ...}]}`,
but `ModelsClient.list_models` expected Codex-specific format
`{models: [{slug: "...", ...}]}`. Deserialization failed silently and models
were ignored.

**Fix** (`codex-api/src/endpoint/models.rs`): Added a fallback in
`list_models`: try Codex format first; on failure, parse as OpenAI format and
map `id→slug`, filling defaults for all other `ModelInfo` fields.

#### 3. Full-history forks skipped skill routing

v2 `spawn_agent` defaults to `fork_turns: "all"` → `FullHistory` mode. In the
fork branch, `reject_full_fork_spawn_overrides` checked for user-explicit
overrides but the `else` branch (which applied the skill-routed model) was
never reached.

**Fix** (`multi_agents_v2/spawn.rs`, `multi_agents/spawn.rs`): Moved
`apply_requested_spawn_agent_model_overrides(skill_model_route...)` to
execute BEFORE the fork-vs-normal branch, so the skill-routed model is
applied to ALL spawns regardless of fork mode.

#### 4. Turn-level skill mention not detected (CURRENT BUG)

The routing fallback relies on `collect_explicit_skill_mentions` finding
`@test-routing` in the spawn_agent input text, OR finding a
`UserInput::Skill` item in the turn's input. `UserInput::Skill` items are
created by the TUI when the user types `@skill-name`, but diagnostic logs
show `@test-routing` arrives at `run_turn` as `UserInput::Text`, not
`UserInput::Skill`.

The turn-level `mentioned_skill_paths` in `TurnSkillsContext` is populated
from `collect_explicit_skill_mentions` which uses
`select_skills_from_mentions`. The second loop (plain name match) requires
`skill_count == 1 && connector_count == 0`. Current diagnostics suggest the
name match itself may be failing.

**Diagnostics in place** (`/tmp/codex-subagent-route.log`):
- `[INPUT]` — raw items reaching `run_turn`
- `[TURN]` — turn-level mentioned skills
- `[SPAWN]` — spawn-level turn paths and skills list
- `[MENTION_DEBUG]` — what `extract_tool_mentions` found in the text
- `[DECISION]` — routing decision with available models

### Remaining work

- Fix the `@test-routing` mention detection so turn-level skill paths are
  populated, or change the routing approach to read active skills from
  session/turn context directly.
- Once routing triggers, verify the profile-model vs available-model matching
  works end-to-end with namespaced provider IDs.
