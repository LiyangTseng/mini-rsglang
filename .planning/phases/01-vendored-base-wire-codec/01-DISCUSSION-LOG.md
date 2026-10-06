# Phase 1: Vendored Base & Wire Codec - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-03
**Phase:** 01-vendored-base-wire-codec
**Areas discussed:** Vendor layout & tracking, Launcher shape & topology, Handshake mechanism, Fixtures & decoder check

---

## Vendor layout & tracking

| Question | Options | Selected |
|----------|---------|----------|
| What gets vendored | Whole repo / Python package + license only / Package + benchmark + tests | Whole repo |
| Location | vendor/mini-sglang/ / Repo root / python/ (merged) | vendor/mini-sglang/ |
| Modification tracking | UPSTREAM.md + check script / UPSTREAM.md only / Pristine import commit + git diff | UPSTREAM.md + check script |
| Frozen-frontend protection | Enforce via check script / Convention only / You decide | Enforce via check script |

---

## Launcher shape & topology

| Question | Options | Selected |
|----------|---------|----------|
| Launch command / parent | Python launcher / Rust binary is parent / Shell wrapper | Python launcher |
| Socket name pinning | Launcher sets _unique_suffix / Fixed suffix / Per-run socket dir | Launcher sets _unique_suffix |
| Who binds minisgl_1 | Mirror upstream Python mode / Scheduler binds both / You decide | Mirror upstream Python mode |
| Phase 1 Rust binary | Skeleton connects + logs handshake / Skeleton + health endpoint | Skeleton connects + logs handshake |

---

## Handshake mechanism

| Question | Options | Selected |
|----------|---------|----------|
| Where produced | Launcher-side wrapper / Patch vendored scheduler | Launcher-side wrapper |
| Channel to Rust | stdin JSON line / CLI args after ready / Dedicated ZMQ message | stdin JSON line |
| Payload | Static via CLI + dynamic via stdin / All in handshake / Only 4 fields | Static via CLI + dynamic via stdin (+ num_pages + upstream SHA) |
| Failure handling | Launcher owns + stdin EOF backstop / Launcher only / Independent timeouts | Launcher owns + stdin EOF backstop |

**Notes:**
- The user asked for an analysis of the payload and failure options before choosing.
- Key reasoning for the payload split: Rust starts in parallel with the backend, so the static socket and model info must be available at spawn time.
- The SHA check catches wire-version mismatches that would otherwise cause a silent ZMQ hang.
- Process-group kill prevents leaked GPU processes from skewing benchmarks.

---

## Fixtures & decoder check

| Question | Options | Selected |
|----------|---------|----------|
| Where generated | Mac + CPU torch uv env / GPU box only | Mac + CPU torch uv env |
| Storage | Commit + Python regen diff / Commit only / Regenerate every test | Commit + Python regen diff |
| WIRE-02 mechanism | Rust dump → pytest decode / cargo test spawns Python | Rust dump → pytest decode |
| Case matrix | Batch variants + numeric widths + tensor bounds, real sequences deferred / Only batch + numeric / All four | Batch + numeric widths + tensor bounds; real sequences deferred to Phase 6 |

**Notes:**
- The user asked for an evaluation of the case categories.
- The float64-vs-f32 issue was highlighted: it would silently change sampling values (for example `top_p`), and this matrix catches it in Phase 1.
- The empty-prompt case was moved to a Phase 5 validation rule.

---

## Claude's Discretion

- Rust workspace and crate layout, and the toolchain pin.
- Hand-rolled rmp vs rmp-serde (as long as the fixtures match byte-for-byte).
- The ZMQ crate for the skeleton (the full decision is in Phase 3).
- Script, command and fixture-format naming.
- uv env and lockfile details.

## Deferred Ideas

- Real upstream message-sequence fixtures, in Phase 6 on the GPU box.
- Empty `input_ids` rejection, as a Phase 5 validation rule.
