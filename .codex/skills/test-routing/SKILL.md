---
name: test-routing
description: Test skill-based model routing. Delegates research tasks to subagents via spawn_agent so the routing system selects the optimal model from model-profiles.toml based on task tags.
---

# Test Routing

A simple test skill to verify subagent model routing. When invoked, it delegates a research task to a subagent — the routing system should match the skill's tags against `model-profiles.toml` and pick the best model.

## Subagents

Uses spawn_agent for the research sub-task.
