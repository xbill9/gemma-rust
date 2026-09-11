---
title: "Two Rust Clients for Gemma 4: Calling the Endpoint vs. Calling the MCP Server 🦀"
published: false
description: "Step by step: a Rust HTTP client (reqwest) and a Rust MCP client (rmcp) ask Gemma 4 E2B the same question on a local llama.cpp GPU and on Cloud Run — what each one can see, and what each one costs."
tags: rust, mcp, gemma, llamacpp
cover_image: https://raw.githubusercontent.com/xbill9/gemma-rust/main/docs/devto-cover.cf7b1a72.jpg
---

This article provides a step by step guide to two small Rust CLIs that ask a self-hosted Gemma 4 E2B the same question. The first calls the model's OpenAI-compatible HTTP endpoint directly. The second is an MCP client: it launches the rig's own MCP server and asks through its tools.

https://github.com/xbill9/gemma-rust

https://github.com/xbill9/gemma-rust-mcp

---

#### What is this project trying to Do?

Both CLIs are demos, and their output is read by an audience. So neither hides anything behind a `--verbose` flag: every run prints the target, the health check, the request, the answer, the model's reasoning, token counts, latency, and whatever the server says about itself.

They run against two very different deployments of the same model with the same code:

- **local**: llama.cpp's `llama-server` on a 2021-era laptop GPU, a GTX 1650 Ti with 4 GiB, no auth
- **Cloud Run**: vLLM on an NVIDIA L4, behind Google Cloud IAM

The interesting part is what changes when the same question goes through MCP instead of HTTP. It is not the answer.

---

#### Why Two Clients?

Because they answer two different questions.

**gemma-rust shows what the model said.** One HTTP call, the raw OpenAI-style response, every field printed.

**gemma-rust-mcp shows what an agent sees.** An MCP client like Claude Code never touches the endpoint. It calls tools, and gets back whatever those tools choose to report. Writing a second client in Rust — one that is not Claude Code and not the Python SDK the servers were built with — is the fastest way to find out what those servers actually return.

Neither one starts, stops or deploys anything. The rigs do that.

---

#### How Does This All Fit Together?

```plaintext
  gemma-rust ─────── HTTP (reqwest) ───────────────────────┐
                                                           ├──▶  llama-server   GTX 1650 Ti, local
                                                           │     vLLM           NVIDIA L4, Cloud Run
  gemma-rust-mcp ─── MCP over stdio (rmcp) ──▶ server.py ──┘
                                               (Python, the rig's own)
```

The MCP path makes the same HTTP call in the end. It just makes it from inside a Python process that the Rust client launched, and hands back markdown instead of JSON.

---

#### Where do I start?

The strategy for building the two clients is an incremental step by step approach.

First, a model server is brought up locally and checked with `curl`. Then the HTTP client is built and validated against it, including the two ways Gemma 4 returns an empty answer from a healthy server. The same binary is then pointed at Cloud Run.

Then the rig's Python MCP server is installed, the MCP client is built, and the same question goes through the same two servers again — which is where the comparison comes from.

---

#### At This Point You Should Have…

- A Linux machine with an NVIDIA GPU, a working driver and the CUDA toolkit (`nvcc`) — this one is a GTX 1650 Ti, CUDA 13.3
- `git`, `cmake`, a C++ compiler, and `curl`
- Python 3 — for the rig's MCP server, not for the Rust clients
- A Hugging Face account that has accepted the Gemma license, and the `hf` CLI
- Optional: the Google Cloud SDK and a Gemma 4 Cloud Run service, for the cloud half. [This article](https://dev.to/gde/2b-gemma-4-deployment-with-cloud-run-nvidia-l4-mcp-sdk-2x-and-claude-code-4ml3) deploys one.

---

#### Step 1 — Install Rust

Use `rustup`:

```shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustc --version
```

```plaintext
rustc 1.98.1 (48a229cea 2026-09-01)
```

Anything recent works. The floors come from the dependencies' own `rust-version`:

| Crate | Needs Rust | Needed by |
|---|---|---|
| `reqwest` 0.13.5 | 1.85.0 | gemma-rust |
| `clap` 4.6.6 | 1.85 | both |
| `rmcp` 3.3.0 | 1.88 | gemma-rust-mcp |

Both crates are edition 2024.

---

#### Step 2 — Build llama.cpp With CUDA

`llama-server` is the local model server. Build it from source, at the commit the rig runs:

```shell
git clone https://github.com/ggml-org/llama.cpp ~/llama.cpp
cd ~/llama.cpp
git checkout 95ef7fc
cmake -B build -DGGML_CUDA=ON -DCMAKE_CUDA_ARCHITECTURES=75 -DCMAKE_BUILD_TYPE=Release
cmake --build build --config Release -j --target llama-server
ls build/bin/llama-server
```

```plaintext
build/bin/llama-server
```

`75` is Turing, which is what a GTX 1650 Ti is. Set your own card's compute capability there, or leave the flag off and let CMake detect it.

---

#### Step 3 — Download the Gemma 4 Checkpoint

The rig serves Google's QAT q4_0 GGUF of Gemma 4 E2B:

```shell
hf auth login
hf download google/gemma-4-E2B-it-qat-q4_0-gguf --local-dir ~/models/gemma-4-E2B-it-qat-q4_0
ls -l ~/models/gemma-4-E2B-it-qat-q4_0/
```

```plaintext
-rw-rw-r-- 1 xbill xbill 3349516256 Sep  3 13:19 gemma-4-E2B_q4_0-it.gguf
```

It fits a 4 GiB card because most of the file never leaves the host — the [previous article](https://dev.to/gde/gemma-4-on-an-old-4-gb-laptop-gpu-qat-takes-it-from-95-gib-to-16-b5l) measures that.

---

#### Step 4 — Start the Model Server

Run it in the foreground; Ctrl-C is the whole teardown:

```shell
~/llama.cpp/build/bin/llama-server \
  -m ~/models/gemma-4-E2B-it-qat-q4_0/gemma-4-E2B_q4_0-it.gguf \
  --host 127.0.0.1 --port 8080 -ngl 99 -c 8192
```

From a second terminal:

```shell
curl -s http://127.0.0.1:8080/health
```

```plaintext
{"status":"ok"}
```

🟢 That is the whole server side for the local target. The rig wraps this same command as `make serve`, with its flags in `tpu.env`.

---

#### Step 5 — Build the HTTP Client

```shell
cd ~
git clone https://github.com/xbill9/gemma-rust
cd gemma-rust
make prod
```

```plaintext
Building release...
    Finished `release` profile [optimized] target(s) in 0.08s
Binary: target/release/gemma-rust
```

(That time is an incremental rebuild; a clean one compiles the dependency tree first.) The dependencies are few:

```toml
[dependencies]
anyhow = "1.0.104"
clap = { version = "4.6.6", features = ["derive", "env"] }
reqwest = { version = "0.13.5", default-features = false, features = ["blocking", "json", "rustls"] }
rustyline = "18.0.1"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
```

**`blocking` is deliberate.** One question, one answer — there is nothing to run concurrently, so there is no async runtime in the client's own code.

Lint is the gate:

```shell
make lint
```

```plaintext
Linting code...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
```

That is `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`. `make test` runs and finds `0 tests`: both crates are demos, validated by running them against live servers, which is what the rest of this article does.

---

#### Step 6 — Ask the Local Model

```shell
./target/release/gemma-rust "In one sentence, what is a TPU?"
```

```plaintext
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
  A TPU (Tensor Processing Unit) is a specialized hardware accelerator designed by Google specifically to speed up the computationally intensive matrix operations required for training and running machine learning models.

== Reasoning =========================================================
  length                 1327 chars
  1.  **Identify the core concept:** The user wants a one-sentence definition of a TPU (Tensor Processing Unit).
  ...

== Stats =============================================================
  finish_reason          stop
  prompt_tokens          25
  cached_tokens          7
  completion_tokens      330
  total_tokens           355
  latency (client)       4786 ms
  tokens/s (client)      69.0  (completion tokens / latency; includes network and prefill)

== Server timings (llama.cpp) ========================================
  predicted_ms           4615.20
  predicted_n            330
  predicted_per_second   71.29
  prompt_ms              147.38
  prompt_n               18
  ...

== Response ==========================================================
  id                     chatcmpl-rTO4HJkYHsrE4WisG2mileq6Jmxn0R4f
  model                  /home/xbill/models/gemma-4-E2B-it-qat-q4_0/gemma-4-E2B_q4_0-it.gguf
  system_fingerprint     b1-95ef7fc
```

✅ A one-sentence answer, and 1,327 characters of thinking in front of it. Gemma 4 on llama.cpp reasons by default, and that is where most of the 330 completion tokens went.

---

#### What the HTTP Client Is Doing

Three decisions make one code path work on two servers that disagree about almost everything.

**The model id comes from the server.** llama.cpp accepts any `model` value; vLLM returns 404 unless it is exactly the served id. The only default that works on both is the first id from `/v1/models`:

```rust
let model = served
    .first()
    .and_then(|m| m["id"].as_str())
    .context("the server listed no models at /v1/models; pass --model")?
    .to_string();
```

**Both reasoning fields are read.** The two servers put Gemma's thinking in different places:

```rust
#[derive(Deserialize)]
struct Message {
    content: Option<String>,
    /// Where llama.cpp puts Gemma 4's thinking
    reasoning_content: Option<String>,
    /// Where vLLM puts it
    reasoning: Option<String>,
}
```

**Server stats are optional and printed generically.** llama.cpp returns a `timings` object; vLLM returns none. Both are `Option<Value>`, and whichever is present gets its own section.

Auth follows the host: `--auth auto` runs `gcloud auth print-identity-token` for `*.run.app` endpoints only, and prints the token's length, never the token.

---

#### 🔎 Tip: Two Ways to Get an Empty Reply From a Healthy Server

**Use `/v1/chat/completions`, never `/v1/completions`.** On these instruction-tuned checkpoints the raw completions endpoint returns empty text, which looks exactly like a broken server.

**Give Gemma room to think.** On llama.cpp, `content` stays empty until the thinking closes. Starve it and see:

```shell
./target/release/gemma-rust --max-tokens 32 "In one sentence, what is a TPU?"; echo "exit=$?"
```

```plaintext
== Request ===========================================================
  POST                   http://127.0.0.1:8080/v1/chat/completions
  prompt                 In one sentence, what is a TPU?
  max_tokens             32
  warning: below 512, Gemma 4 may still be reasoning when it hits the limit and return an empty answer

== Answer ============================================================
  (empty) The model was still reasoning when it stopped. This is Gemma 4 thinking, not a broken server: raise --max-tokens.

== Reasoning =========================================================
  length                 116 chars
  1.  **Analyze the Request:** The user wants a definition of a TPU (Tensor Processing Unit) in a *single sentence*.
  2

== Stats =============================================================
  finish_reason          length
  The reply hit max_tokens and is cut off.
exit=2
```

`finish_reason: length`, empty `content`, non-empty reasoning. The CLI says what happened and **exits 2**, so a script cannot mistake an empty answer for success. That is why `--max-tokens` defaults to 1024.

---

#### What Does the Server Say About Itself?

`--status` skips the question and probes every endpoint either server might offer:

```shell
./target/release/gemma-rust --status
```

```plaintext
== Server ============================================================
  GET /version           404 Not Found (not served by this server)
  GET /props             200 OK in 0 ms
  llama.cpp build        b1-95ef7fc
  model_ftype            Q4_0
  n_ctx                  8192
  total_slots            1
  modalities             text only

== Model details =====================================================
  GET /v1/models         200 OK in 0 ms
  n_ctx_train            131072
  n_embd                 1536
  n_params               4628569635 (4.63 B parameters)
  size                   3333699724 (3.33 GB on disk)

== Slots =============================================================
  GET /slots             200 OK in 0 ms
  slots                  1 (0 busy)

== Metrics ===========================================================
  GET /metrics           200 OK in 0 ms
  predicted_tokens_seconds 67.523
  requests_processing    0
  ...
```

**A 404 is reported, not an error.** `/version` is vLLM's; `/props` and `/slots` are llama.cpp's. Each server answers half the probes, and the list of which ones is itself useful.

---

#### Step 7 — Point It at Cloud Run

Same binary, different endpoint. If you deployed the Cloud Run rig, its Makefile prints the URL:

```shell
GEMMA_ENDPOINT=$(make -s -C ~/gemma4-dev/gpu-2B-cloudrun-devops-agent endpoint) \
  ./target/release/gemma-rust "In one sentence, what is a TPU?"
```

```plaintext
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

🟢 31 tokens and no reasoning — vLLM does not think by default here — so the round trip is 662 ms. The service is `--no-allow-unauthenticated`; without a token the CLI explains itself instead of dumping Google's error page:

```plaintext
  GET /health            403 Forbidden in 304 ms
  body                   (299 bytes, not shown)
  The server rejected the request's credentials. Cloud Run needs an identity token from an account with roles/run.invoker (`gcloud auth print-identity-token`).
```

What the one code path had to absorb:

| | local llama.cpp | Cloud Run vLLM 0.26 |
|---|---|---|
| auth | none | identity token (403 without) |
| `model` field | any value | exact served id (404 otherwise) |
| reasoning field | `reasoning_content` | `reasoning` |
| thinks by default | yes | no |
| server stats | `timings` object | none |
| context | 8192 | 16384 |

---

#### Step 8 — Install the Rig's MCP Server

The MCP client does not talk to the model. It launches a rig's `server.py`, so the rigs come next:

```shell
git clone https://github.com/xbill9/gemma4-dev ~/gemma4-dev
make -C ~/gemma4-dev/local-llamacpp-1650ti-2b-q4_0 install
python3 -c "import importlib.metadata as m;print('mcp', m.version('mcp'))"
```

```plaintext
mcp 2.2.0
```

The rig servers need `mcp>=2` — the Python SDK line where `FastMCP` became `MCPServer`. They install into the system `python3`; if yours refuses system-wide installs, use a virtualenv and point the client at it with `--python` or `GEMMA_PYTHON`.

The rig reads its model path and `llama-server` location from `tpu.env`. A real environment variable wins over that file, so set `MODEL_PATH` and `LLAMA_SERVER_BIN` if yours live somewhere else.

---

#### Step 9 — Build the MCP Client

```shell
cd ~
git clone https://github.com/xbill9/gemma-rust-mcp
cd gemma-rust-mcp
make prod
```

```plaintext
Building release...
    Finished `release` profile [optimized] target(s) in 0.04s
Binary: target/release/gemma-rust-mcp
```

The feature flags are the part to get right. `rmcp`'s defaults are `base64`, `macros` and `server` — a server's feature set. A client has to ask for `client` and a transport by name:

```toml
[dependencies]
anyhow = "1.0.104"
clap = { version = "4.6.6", features = ["derive", "env"] }
rmcp = { version = "3.3.0", features = ["client", "transport-child-process"] }
rustyline = "18.0.1"
serde = "1.0.229"
serde_json = "1.0.151"
tokio = { version = "1.53.1", features = ["macros", "rt-multi-thread", "process", "time"] }
```

| Feature | What it brings |
|---|---|
| `client` | `ServiceExt::serve` on the client side, `call_tool`, `list_all_tools` |
| `transport-child-process` | `TokioChildProcess`: spawn a server, speak MCP over its stdin/stdout |

`make lint` is clean here too, and `make test` again finds `0 tests`.

---

#### The MCP Client in Four Moves

**Launch the server as a child process.** The rig opens its files by relative path, so the working directory matters. Stderr is piped so the server's own log can be printed at the end:

```rust
let mut cmd = Command::new(&args.python);
cmd.arg("server.py").current_dir(&dir);
let (transport, stderr) = TokioChildProcess::builder(cmd)
    .stderr(Stdio::piped())
    .spawn()?;
```

**Handshake.** The client handler is `()` — this client has no callbacks to offer the server:

```rust
let client = tokio::time::timeout(timeout, ().serve(transport)).await??;
```

**List the tools**, then **call one**:

```rust
let tools = client.list_all_tools().await?;

let params = CallToolRequestParams::new(name).with_arguments(arguments);
let result = client.call_tool(params).await?;
```

💡 `CallToolRequestParams` is `#[non_exhaustive]`, so a struct literal will not compile. Use the constructor and the builder method.

---

#### Step 10 — Ask Through MCP

```shell
./target/release/gemma-rust-mcp "In one sentence, what is a TPU?"
```

```plaintext
== MCP server ========================================================
  rig                    local-llamacpp-1650ti-2b-q4_0
  command                python3 server.py
  working dir            /home/xbill/gemma4-dev/local-llamacpp-1650ti-2b-q4_0
  transport              stdio (child process)
  pid                    334541
  initialize             ok in 793 ms

== Server info =======================================================
  name                   local-llamacpp-1650ti-2b-q4_0
  version                (empty)
  protocol               2025-11-25
  capabilities           tools, resources, prompts

== Tools =============================================================
  tools/list             7 tools in 2 ms
  * gpu_status           Report the local GPU: name, compute capability, VRAM total/…
  * model_info           Report the configured checkpoint, where it is, and the resi…
    start_model_server   Start llama-server on the local GPU. No-op if it is already…
    stop_model_server    Stop the running llama-server. Teardown is complete — nothi…
  * model_server_status  Check whether llama-server is up and serving at the known l…
  * query_model          Send a chat completion to the local endpoint and return the…
    get_help             List the tools this rig exposes.
  (* = called by this demo, which only calls read-only tools)

== tools/call gpu_status =============================================
  arguments              {}
  latency                14 ms
  isError                false
  result:
    📡 **GPU** — `local-llamacpp-1650ti-2b-q4_0`
    NVIDIA GeForce GTX 1650 Ti with Max-Q Design, 7.5, 4096 MiB, 1632 MiB, 2101 MiB, 615.71.09

== tools/call model_server_status ====================================
  latency                27 ms
  result:
    ✅ Serving at http://127.0.0.1:8080 (pid 83619). `/health` → 200.

== tools/call query_model ============================================
  arguments              {"max_tokens":1024,"prompt":"In one sentence, what is a TPU?"}
  latency                4608 ms
  isError                false
  result:
    ✅ **Reply**

    A TPU (Tensor Processing Unit) is a specialized integrated circuit developed by Google designed to accelerate machine learning workloads, specifically the complex matrix multiplications required by neural networks, significantly speeding up training and inference.

    ---
    _(plus 1173 chars of reasoning, suppressed)_
    prompt 25 tok · completion 326 tok · 71.3 tok/s

== Server log (stderr) ===============================================
  2026-09-11 16:27:09,682 INFO HTTP Request: GET http://127.0.0.1:8080/health "HTTP/1.1 200 OK"
  2026-09-11 16:27:14,292 INFO HTTP Request: POST http://127.0.0.1:8080/v1/chat/completions "HTTP/1.1 200 OK"
```

✅ Same model, same kind of answer. What came back around it is completely different: the GPU and its memory, the server's health, and a reply formatted for an agent to read — with the reasoning reduced to its length.

The server log at the bottom shows the HTTP call the tool made on the client's behalf. That is the "same servers" arrow in the diagram, visible.

---

#### Only Read-Only Tools — a Hard Rule

The rig servers also offer tools that start and stop the model server, and on Cloud Run, deploy, destroy and rescale a billed GPU service. The client calls a fixed list and nothing else:

```rust
/// Read-only tools called before the query. Never add a tool that deploys, destroys,
/// scales, starts, or stops anything: those are billed or destructive.
fn status_tools(self) -> &'static [&'static str] {
    match self {
        Rig::Local => &["gpu_status", "model_server_status", "model_info"],
        Rig::Cloudrun => &[
            "cloudrun_status",
            "cloudrun_get_system_status",
            "cloudrun_get_model_details",
        ],
    }
}
```

There is no "call any tool" option, on the command line or in interactive mode. A demo that can be talked into `cloudrun_destroy` is not a demo anyone should run in front of an audience.

---

#### 🔎 Tip: Failure Is in the Text, Not the Protocol

MCP has an `isError` flag on every tool result. These rig tools never set it. They report failure as markdown that starts with `❌`, and a reasoning-only reply as markdown that starts with `📡`:

```shell
./target/release/gemma-rust-mcp --no-status --max-tokens 32 "In one sentence, what is a TPU?"; echo "exit=$?"
```

```plaintext
== tools/call query_model ============================================
  arguments              {"max_tokens":32,"prompt":"In one sentence, what is a TPU?"}
  latency                499 ms
  isError                false
  result:
    📡 **Reasoning only — no answer yet.** `finish_reason: length` after 32 tokens, all of them thinking.

    This is Gemma 4 reasoning, not a broken server. Re-run with a larger `max_tokens` (currently 32).
    ...
  No answer: the model was still reasoning when it stopped. Raise --max-tokens.
exit=2
```

`isError false`, and still not an answer. A client that trusts the protocol flag reports success here. This one reads the first character of the text, and exits 2 exactly like the HTTP client does.

---

#### MCP Against Cloud Run

```shell
./target/release/gemma-rust-mcp --rig cloudrun "In one sentence, what is a TPU?"
```

```plaintext
== MCP server ========================================================
  rig                    gpu-2B-cloudrun-devops-agent
  initialize             ok in 3219 ms

== Server info =======================================================
  name                   Self-Hosted vLLM DevOps Agent
  protocol               2025-11-25

== Tools =============================================================
  tools/list             27 tools in 2 ms
  ...

== tools/call cloudrun_query_gemma4_with_stats =======================
  arguments              {"prompt":"In one sentence, what is a TPU?"}
  latency                1333 ms
  isError                false
  result:
    ### 📊 Performance Stats
    - **Model:** `/mnt/models/gemma-4-E2B-it`
    - **Time to First Token (TTFT):** `0.086s`
    - **Total Generation Time:** `0.688s`
    - **Tokens per Second:** `53.11 tokens/s`
    - **Total Tokens (approx.):** `32`

    ### 💬 Model Response
    A TPU (Tensor Processing Unit) is a specialized type of integrated circuit designed to accelerate machine learning workloads, particularly those involving tensor operations common in deep learning.<turn|>

== Server log (stderr) ===============================================
  ... INFO - 📡 Automatically discovered vLLM at: https://<your-service>.a.run.app
  2026-09-11 11:41:36,754 - httpx - INFO - HTTP Request: GET https://<your-service>.a.run.app/health "HTTP/1.1 200 OK"
  2026-09-11 11:41:37,399 - httpx2 - INFO - HTTP Request: GET https://<your-service>.a.run.app/v1/models "HTTP/1.1 200 OK"
  2026-09-11 11:41:37,479 - httpx2 - INFO - HTTP Request: POST https://<your-service>.a.run.app/v1/chat/completions "HTTP/1.1 200 OK"
```

Three things the HTTP client could not have shown:

- **TTFT.** The tool streams, so it measures time to first token — 0.086 s. The HTTP client makes one non-streaming call and sees only the total.
- **The cost of the extra hop.** The server log shows the tool doing a `GET /health` and a `GET /v1/models` before its `POST`. The HTTP client does those once per session; this tool does them on every call.
- **A bug, in the server.** The answer ends in `<turn|>`, Gemma's end-of-turn marker, leaked by the rig's streaming tool. The client prints what the tool returned — which is how the bug was found.

---

#### Compare and Contrast

The same question, through the same two servers:

| | 🦀 gemma-rust (HTTP) | 🦀 gemma-rust-mcp (MCP) |
|---|---|---|
| What you get | 🥇 the raw OpenAI-style response | what the tool chooses to report, as markdown |
| Reasoning | 🥇 full text | local: its length only; Cloud Run: none |
| Token counts | 🥇 exact, from `usage`, incl. cached | local: exact; Cloud Run: approximate |
| Rig status | HTTP probes of the server | 🥇 GPU, model and deployment, from the rig's tools |
| `-i` keeps the conversation | 🥇 yes | no — the tool takes one prompt |
| Runs on its own | 🥇 yes | needs Python 3, `mcp>=2` and the rig's `server.py` |

And what it cost, measured:

| | 🦀 gemma-rust (HTTP) | 🦀 gemma-rust-mcp (MCP) |
|---|---|---|
| Local query | 4786 ms, 330 tokens | 4608 ms, 326 tokens |
| Cloud Run query | 🥇 662 ms | 1333 ms |
| Before the first query, Cloud Run | 🥇 185 ms `GET /health`, plus the token fetch | 3219 ms `initialize` |
| Crates in `Cargo.lock` | 186 | 🥇 121 |
| Release binary | 8.5M | 🥇 6.5M |
| `src/main.rs` | 777 lines | 🥇 542 lines |

**Locally, it is a tie.** 4786 ms for 330 tokens against 4608 ms for 326: the model's thinking dominates both, and one sample each cannot separate the MCP layer from the difference between one Gemma reply and the next.

**On Cloud Run, the hop shows.** 1333 ms against 662 ms is 671 ms (arithmetic), roughly the two extra round trips the tool makes before it asks. Session start is where MCP really pays: 3219 ms to launch Python, import the SDK and discover the service URL through `gcloud`, before the first tool call.

**The smaller crate is the MCP one**, because it speaks stdio and never opens a TLS connection. `reqwest` with `rustls` brings a TLS stack; the MCP client leaves the HTTPS to the Python server.

---

#### So, Which One?

**Use the HTTP client to see the model.** Reasoning, exact token counts, cached tokens, server timings, the raw JSON with `--raw`. When the question is "what did Gemma do", this is the one.

**Use the MCP client to see the agent's view.** It is the fastest way to test an MCP server from outside the SDK it was written with, and it found two things no unit test did: a failure flag that is never set, and an end-of-turn marker leaking into answers.

Neither half of that is about Rust being fast. The work is a model thinking on a GPU. Rust earns its place here with one static binary per client, a type for every response field, and a compiler that notices when vLLM and llama.cpp disagree about where the reasoning goes.

---

#### Summary

The goal of this article was to ask Gemma 4 E2B the same question from Rust in two ways — directly over its HTTP endpoint, and through the rig's MCP server — on a local llama.cpp GPU and on Cloud Run. The key to the solution was printing everything each path returns, which turns two small clients into a side-by-side view of what a model says and what an agent sees. The results were:

- 🟢 One HTTP code path served both targets by taking the model id from `/v1/models` and reading both reasoning fields
- 🟢 Both clients report an empty answer from a thinking-starved Gemma 4 as exit 2, not as success
- 🟢 The MCP client added GPU, model and deployment status, plus TTFT on Cloud Run, from the rig's own tools
- ❌ The MCP path loses the reasoning text and, on Cloud Run, exact token counts
- ⚠️ On Cloud Run the MCP query took 1333 ms against 662 ms over HTTP, and 3219 ms to initialize
- ⚠️ The rig tools report failure in text with `isError` false, and the Cloud Run tool leaks `<turn|>`

Scope: one laptop with a GTX 1650 Ti Max-Q (4 GiB, driver 615.71.09) running llama.cpp `b1-95ef7fc` with the Gemma 4 E2B QAT q4_0 GGUF, and one Cloud Run service on an NVIDIA L4 running vLLM 0.26.0. One run per client per target, so each figure is a single sample and model replies vary from run to run; the local runs were captured at 16:27 on 2026-09-11 and the Cloud Run runs earlier the same day, and the HTTP and MCP runs are separate requests, not the same one observed twice. Rust 1.98.1, `rmcp` 3.3.0, `reqwest` 0.13.5, Python 3.14.7 with `mcp` 2.2.0.

The strategy for using Rust to call Gemma 4 over HTTP and over MCP was validated with an incremental step by step approach.

#### References

* [gemma-rust | GitHub](https://github.com/xbill9/gemma-rust)
* [gemma-rust-mcp | GitHub](https://github.com/xbill9/gemma-rust-mcp)
* [gemma4-dev rigs | GitHub](https://github.com/xbill9/gemma4-dev)
* [rmcp — the official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
* [llama.cpp](https://github.com/ggml-org/llama.cpp)
* [Gemma 4 on an 2021 4 GB Laptop GPU: QAT Takes It From 9.5 GiB to 1.6](https://dev.to/gde/gemma-4-on-an-old-4-gb-laptop-gpu-qat-takes-it-from-95-gib-to-16-b5l)
* [2B Gemma 4 Deployment with Cloud Run, NVIDIA L4, MCP SDK 2.x, and Claude Code](https://dev.to/gde/2b-gemma-4-deployment-with-cloud-run-nvidia-l4-mcp-sdk-2x-and-claude-code-4ml3)
