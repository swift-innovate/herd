# Herd Roadmap

**Updated:** March 5, 2026

## Vision

Herd is evolving from an intelligent Ollama router into the **complete local AI orchestration platform**.

One fast, single Rust binary will give you:
- GPU-aware routing across multiple Ollama nodes
- Safe, production-grade multi-agent orchestration with strong permission guardrails
- Unified observability and dashboard

No more running separate services. No API keys exposed. Full local control.

## Why Merge Conduit into Herd?

Instead of maintaining two separate projects, we're bringing the best ideas from Conduit (session management, permission engine, audit logging, and multi-agent patterns) directly into Herd.

Benefits:
- Single binary (`herd`) and single port
- Deep integration between GPU routing and agent execution
- Faster development and better consistency
- Zero Anthropic ToS risk (we're building on local Ollama with tool-use guardrails)

The Conduit repository will be archived with a clear redirect here.

## Roadmap

### Phase 1 — v0.3.0: Agent Gateway (Target: Late March 2026)
**Core Multi-Agent Support**

- New `/agent/` REST + WebSocket API
- Session management (create, list, resume, delete)
- Permission engine with configurable deny rules (file I/O and bash execution guardrails)
- Built-in audit logging (extends existing `requests.jsonl`)
- Full integration with existing router, GPU scoring, and circuit breaker
- Basic tool calling support for Ollama models
- New "Sessions" tab in the dashboard

**Backward compatible** — all existing `/api` and future `/v1` endpoints continue to work unchanged.

### Phase 2 — v0.4.0: Enhanced Orchestration (April 2026)
- Advanced permission profiles and project-based scoping
- Real-time session monitoring and cost estimation
- WebSocket streaming improvements
- OpenAI `/v1/chat/completions` compatibility layer (full drop-in support)
- Model-aware agent routing (send agentic workloads to best GPU node)

### Phase 3 — v0.5.0+: Multi-Model & Advanced Features (Q2 2026)
- Commercial backend support (Anthropic, OpenAI, etc. via API keys)
- MCP server compatibility
- Session forking and templating
- Distributed agent coordination across nodes
- Plugin system for custom tools

## Technical Approach

All new agent functionality will live in a clean `agent/` module that reuses Herd's existing architecture:
- `RouterEngine` for intelligent backend selection
- `CircuitBreaker` for resilience
- Existing observability and hot-reload config
- Minimal new dependencies

## Get Involved

This is the biggest evolution of Herd yet.

If you're interested in:
- Testing early agent builds
- Contributing to the permission engine
- Sharing real-world agent use cases

...please open an issue or discussion with the label `agent-gateway`.

The goal is simple: Make Herd the best place to run both powerful local models **and** safe, controllable agents on your own hardware.

— swift-innovate
