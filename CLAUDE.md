# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A small Rust CLI (`src/main.rs`) that sends one prompt to a Gemma 4 E2B endpoint and prints the
reply. It targets either the local llama.cpp rig (default) or the Cloud Run vLLM service.
**It is a demo** — its output is read by an audience, so detailed output is a requirement (see
"Demo output" below), not something to hide behind a `--verbose` flag.

The CLI only *calls* servers. It never starts, stops, deploys, or configures them — the rigs do:

- local: `make -C ~/gemma4-dev/local-llamacpp-1650ti-2b-q4_0 serve` (foreground; Ctrl-C is teardown), `... status`
- Cloud Run: `make -C ~/gemma4-dev/gpu-2B-cloudrun-devops-agent status|endpoint`. `deploy`/`destroy`
  there are billed GPU operations — never run them unless asked.

Each rig's `CLAUDE.md` is the reference for how its server behaves.

## Running

- `cargo run -- "prompt"` hits the local rig. `--endpoint`/`GEMMA_ENDPOINT` selects another server,
  e.g. `GEMMA_ENDPOINT=$(make -s -C ~/gemma4-dev/gpu-2B-cloudrun-devops-agent endpoint) cargo run -- "prompt"`.
- `--auth auto` (default) fetches `gcloud auth print-identity-token` for `*.run.app` hosts only;
  `--token`/`GEMMA_TOKEN` supplies one directly. Never print the token.
- Exit codes: 0 answered, 1 error, 2 empty answer.

## The two targets differ — measured 2026-09-11

| | local llama.cpp | Cloud Run vLLM 0.26 |
|---|---|---|
| endpoint | `http://127.0.0.1:8080` (`ENDPOINT` in the rig's `tpu.env`) | `make -s ... endpoint` (a `*.run.app` URL) |
| auth | none | identity token required (`--no-allow-unauthenticated`); 403 without |
| `model` field | any value accepted | must be exactly `/mnt/models/gemma-4-E2B-it`; 404 otherwise |
| reasoning field | `message.reasoning_content` | `message.reasoning` |
| thinks by default | yes — hundreds of tokens | no — reasoning came back empty |
| server stats | `timings` object | none — the `metrics` key is present but `null` |
| context | 8192 | 16384 |

Consequences, already applied in `main.rs`: the model id defaults to the first id from
`/v1/models` (the only default that works on both); both reasoning fields are read; `timings` and
`metrics` are optional and printed generically. Cloud Run scales to zero, so the first request can
take minutes — the default timeout is 600 s.

Never derive a host, port, or model from a rig's directory name; read the rig's env/Makefile.

## Calling the model: two ways to get an empty reply

- Use `POST /v1/chat/completions`. **Never `/v1/completions`** — on these `-it` checkpoints it
  returns an empty completion, which looks like a broken server and is not one.
- On llama.cpp, Gemma 4 reasons before answering and `content` stays empty until the thinking
  block closes, so a small `max_tokens` gives `content: ""` with `finish_reason: "length"` on a
  healthy server. Default `max_tokens` is **1024; keep it ≥ 512**. When `content` is empty and the
  reasoning is not, say so explicitly; never present an empty answer as success.

## Demo output

Every run prints, in labelled sections: target, endpoint and auth source; `/health` status and
latency; models served with their context size; the request; the answer; the reasoning with its
character count; `finish_reason`; `usage` token counts (incl. `prompt_tokens_details.cached_tokens`);
client-measured latency and tokens/s; the server's `timings` or `metrics`; and `id`, `model`,
`system_fingerprint`. `--raw` adds the full JSON. Keep new output in that style; if a server
returns more useful detail, print it rather than drop it.

If the local server refuses the connection, say it is not running and print the `make ... serve`
command rather than a raw error; on 401/403, explain the identity-token requirement.
