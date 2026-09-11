# gemma-rust

A small Rust CLI that asks a Gemma 4 model one question over an OpenAI-compatible HTTP API and
prints **everything** the server says about the answer: health check, served models, the request,
the answer, the model's reasoning, token usage, client latency, and the server's own timings.

It was built as a demo. It works with two very different deployments of Gemma 4 E2B using the same
code path:

- **local**: llama.cpp's `llama-server` on a laptop GPU (GTX 1650 Ti), no auth
- **Cloud Run**: vLLM on an NVIDIA L4, behind Google Cloud IAM

## Build

Requires Rust 1.85+ (edition 2024).

```sh
cargo build --release
./target/release/gemma-rust --help
```

## Usage

```sh
# Local llama.cpp server on http://127.0.0.1:8080 (the default)
gemma-rust "In one sentence, what is a TPU?"

# Cloud Run: an identity token is fetched with gcloud automatically for *.run.app hosts
GEMMA_ENDPOINT=https://<your-service>.a.run.app gemma-rust "In one sentence, what is a TPU?"

# Also dump the raw JSON response
gemma-rust --raw "Why is the sky blue?"
```

| Option | Env | Default | |
|---|---|---|---|
| `-e, --endpoint` | `GEMMA_ENDPOINT` | `http://127.0.0.1:8080` | Base URL of the server |
| `-m, --model` | `GEMMA_MODEL` | first id from `/v1/models` | Model id to request |
| `--auth` | | `auto` | `auto` (gcloud identity token for `*.run.app` only), `gcloud`, or `none` |
| `--token` | `GEMMA_TOKEN` | | Bearer token to send instead of asking gcloud |
| `--max-tokens` | | `1024` | Keep ≥ 512 (see below) |
| `--temperature` | | server's | Sampling temperature |
| `--system` | | | System prompt |
| `--timeout` | | `600` | Seconds; Cloud Run can take minutes to cold-start a GPU |
| `--raw` | | | Also print the raw JSON response |

Exit codes: `0` answered, `1` error, `2` the model returned an empty answer.

## Setting up a server

**Local, llama.cpp.** Any recent `llama-server` build with a Gemma 4 GGUF:

```sh
llama-server -m gemma-4-E2B_q4_0-it.gguf --host 127.0.0.1 --port 8080 -ngl 99
```

**Cloud Run, vLLM.** Deploy the `vllm/vllm-openai` image with a GPU, `--reasoning-parser=gemma4`,
and `--no-allow-unauthenticated`. The caller needs `roles/run.invoker` and the Google Cloud SDK
(`gcloud auth print-identity-token`).

## Things that differ between the two servers

| | local llama.cpp | Cloud Run vLLM 0.26 |
|---|---|---|
| auth | none | identity token required (403 without) |
| `model` field | any value accepted | must match the served id exactly (404 otherwise) |
| reasoning field | `message.reasoning_content` | `message.reasoning` |
| thinks by default | yes, hundreds of tokens | no |
| server stats | `timings` object | none |
| context | 8192 | 16384 |

That is why the CLI defaults the model id to whatever `/v1/models` lists, reads both reasoning
fields, and treats server timings as optional.

**Why `--max-tokens` defaults to 1024.** On llama.cpp, Gemma 4 thinks before it answers, and
`content` stays empty until the thinking finishes. With a small limit a healthy server returns an
empty answer with `finish_reason: "length"`. The CLI detects that case and says so instead of
printing nothing. Also, always use `/v1/chat/completions`: the raw `/v1/completions` endpoint
returns empty text for these instruction-tuned checkpoints.

## Sample output

Measured 2026-09-11 with the same prompt against both targets.

### Local: llama.cpp on a GTX 1650 Ti

```text
== Target ============================================================
  endpoint               http://127.0.0.1:8080
  target                 local (llama.cpp rig)
  auth                   none

== Health ============================================================
  GET /health            200 OK in 0 ms
  body                   {"status":"ok"}

== Model =============================================================
  served                 /home/xbill/models/gemma-4-E2B-it-qat-q4_0/gemma-4-E2B_q4_0-it.gguf (context 8192 tokens)
  using                  /home/xbill/models/gemma-4-E2B-it-qat-q4_0/gemma-4-E2B_q4_0-it.gguf (first model the server lists)

== Request ===========================================================
  POST                   http://127.0.0.1:8080/v1/chat/completions
  prompt                 In one sentence, what is a TPU?
  max_tokens             1024

== Answer ============================================================
  A TPU (Tensor Processing Unit) is a specialized hardware accelerator designed to rapidly perform the matrix multiplication operations essential for training and running deep learning models.

== Reasoning =========================================================
  length                 863 chars
  Thinking Process:
  
  1.  **Analyze the Request:** The user wants a definition of "TPU" in exactly one sentence.
  2.  **Identify the Term:** What does TPU stand for in the context of technology/AI?
      *   TPU = Tensor Processing Unit.
  3.  **Determine the Function/Purpose (Core Concept):** What is a TPU used for?
      *   It's a specialized hardware accelerator designed to accelerate the matrix operations common in deep learning (neural networks).
  4.  **Draft the Sentence (Focusing on clarity and brevity):**
      *   *Draft 1:* A TPU is a specialized hardware accelerator that is designed to speed up the matrix multiplication operations needed for deep learning models.
  5.  **Refine and Finalize (Ensuring it's a single, strong sentence):** The draft is accurate and fits the constraint.
  
  6.  **Final Output Generation.** (This matches the provided good answer.)

== Stats =============================================================
  finish_reason          stop
  prompt_tokens          25
  cached_tokens          20
  completion_tokens      239
  total_tokens           264
  latency (client)       3493 ms
  tokens/s (client)      68.4  (completion tokens / latency; includes network and prefill)

== Server timings (llama.cpp) ========================================
  cache_n                20
  predicted_ms           3422.80
  predicted_n            239
  predicted_per_second   69.53
  predicted_per_token_ms 14.38
  prompt_ms              52.90
  prompt_n               5
  prompt_per_second      94.51
  prompt_per_token_ms    10.58

== Response ==========================================================
  id                     chatcmpl-EHU226Hr5DdAqIIe8hlhAcWyr6Ac4LcK
  model                  /home/xbill/models/gemma-4-E2B-it-qat-q4_0/gemma-4-E2B_q4_0-it.gguf
  system_fingerprint     b1-95ef7fc
```

### Cloud Run: vLLM on an NVIDIA L4

```text
== Target ============================================================
  endpoint               https://<your-service>.a.run.app
  target                 Cloud Run
  auth                   bearer token from gcloud auth print-identity-token (845 chars, not shown)

== Health ============================================================
  Cloud Run scales to zero: the first request can take minutes while a GPU instance starts.
  GET /health            200 OK in 185 ms

== Model =============================================================
  served                 /mnt/models/gemma-4-E2B-it (context 16384 tokens)
  using                  /mnt/models/gemma-4-E2B-it (first model the server lists)

== Request ===========================================================
  POST                   https://<your-service>.a.run.app/v1/chat/completions
  prompt                 In one sentence, what is a TPU?
  max_tokens             1024

== Answer ============================================================
  A TPU (Tensor Processing Unit) is a specialized integrated circuit designed to accelerate machine learning workloads, particularly those involving large matrix multiplications common in deep learning.

== Reasoning =========================================================
  (none returned)

== Stats =============================================================
  finish_reason          stop
  prompt_tokens          18
  cached_tokens          -
  completion_tokens      31
  total_tokens           49
  latency (client)       662 ms
  tokens/s (client)      46.8  (completion tokens / latency; includes network and prefill)

== Response ==========================================================
  id                     chatcmpl-a870c80a585e2371
  model                  /mnt/models/gemma-4-E2B-it
  system_fingerprint     vllm-0.26.0-a3e182ca
```

The local run is slower end to end (3.5 s vs 0.7 s) because Gemma spent 239 tokens reasoning
first; Cloud Run answered in 31 tokens with no reasoning.

### When something is wrong

```text
# No local server running
== Health ============================================================
  cannot connect: error sending request for url (http://127.0.0.1:8099/health): client error (Connect): tcp connect error: Connection refused (os error 111)
  The local llama-server is not running. Start it with:
    make -C ~/gemma4-dev/local-llamacpp-1650ti-2b-q4_0 serve

# Cloud Run without credentials (--auth none)
  GET /health            403 Forbidden in 304 ms
  body                   (299 bytes, not shown)
  The server rejected the request's credentials. Cloud Run needs an identity token from an account with roles/run.invoker (`gcloud auth print-identity-token`).

# --max-tokens 32 on llama.cpp: out of budget while still thinking (exit code 2)
== Answer ============================================================
  (empty) The model was still reasoning when it stopped. This is Gemma 4 thinking, not a broken server: raise --max-tokens.
```
