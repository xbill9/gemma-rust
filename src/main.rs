//! Demo CLI: send one prompt to a Gemma 4 endpoint — the local llama.cpp rig or the Cloud Run
//! vLLM service — and print everything the server says about the answer.

use std::fmt::Display;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde_json::{Value, json};

const LOCAL_SERVE_HINT: &str = "make -C ~/gemma4-dev/local-llamacpp-1650ti-2b-q4_0 serve";

#[derive(Parser)]
#[command(
    version,
    about = "Ask a Gemma 4 endpoint one question and show the details"
)]
struct Args {
    /// Prompt to send
    #[arg(default_value = "In one sentence, what is a TPU?")]
    prompt: String,

    /// Base URL of an OpenAI-compatible server: the local llama.cpp rig or a Cloud Run URL
    #[arg(
        short,
        long,
        env = "GEMMA_ENDPOINT",
        default_value = "http://127.0.0.1:8080"
    )]
    endpoint: String,

    /// Model id [default: the first id the server lists at /v1/models]
    #[arg(short, long, env = "GEMMA_MODEL")]
    model: Option<String>,

    /// How to authenticate
    #[arg(long, value_enum, default_value_t = Auth::Auto)]
    auth: Auth,

    /// Bearer token to send instead of asking gcloud for one
    #[arg(long, env = "GEMMA_TOKEN", hide_env_values = true)]
    token: Option<String>,

    /// Keep this >= 512: Gemma 4 can spend hundreds of tokens reasoning before it answers
    #[arg(long, default_value_t = 1024)]
    max_tokens: u32,

    /// Sampling temperature [default: the server's]
    #[arg(long)]
    temperature: Option<f64>,

    /// Optional system prompt
    #[arg(long)]
    system: Option<String>,

    /// Request timeout in seconds (Cloud Run can take minutes to cold-start a GPU instance)
    #[arg(long, default_value_t = 600)]
    timeout: u64,

    /// Also print the raw JSON response
    #[arg(long)]
    raw: bool,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Auth {
    /// gcloud identity token for *.run.app endpoints, nothing otherwise
    Auto,
    /// Always send a gcloud identity token
    Gcloud,
    /// Never send credentials
    None,
}

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Local,
    CloudRun,
    Other,
}

impl Target {
    fn of(url: &Url) -> Self {
        match url.host_str().unwrap_or_default() {
            h if h.ends_with(".run.app") => Target::CloudRun,
            "127.0.0.1" | "localhost" | "[::1]" => Target::Local,
            _ => Target::Other,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Target::Local => "local (llama.cpp rig)",
            Target::CloudRun => "Cloud Run",
            Target::Other => "other OpenAI-compatible server",
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    id: Option<String>,
    model: Option<String>,
    system_fingerprint: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<Usage>,
    /// llama.cpp only
    timings: Option<Value>,
    /// vLLM only
    metrics: Option<Value>,
}

#[derive(Deserialize)]
struct Choice {
    finish_reason: Option<String>,
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
    /// Where llama.cpp puts Gemma 4's thinking
    reasoning_content: Option<String>,
    /// Where vLLM puts it
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize)]
struct PromptTokensDetails {
    cached_tokens: Option<u64>,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("\nerror: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<ExitCode> {
    let base = args.endpoint.trim_end_matches('/').to_string();
    let url = Url::parse(&base).with_context(|| format!("invalid endpoint {base:?}"))?;
    let target = Target::of(&url);

    section("Target");
    field("endpoint", &base);
    field("target", target.label());
    let token = resolve_token(&args, target)?;
    field(
        "auth",
        match &token {
            Some((source, t)) => {
                format!("bearer token from {source} ({} chars, not shown)", t.len())
            }
            None => "none".to_string(),
        },
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()?;
    let authed = |rb: RequestBuilder| match &token {
        Some((_, t)) => rb.bearer_auth(t),
        None => rb,
    };

    section("Health");
    if target == Target::CloudRun {
        println!(
            "  Cloud Run scales to zero: the first request can take minutes while a GPU instance starts."
        );
    }
    let started = Instant::now();
    let health = match authed(client.get(format!("{base}/health"))).send() {
        Ok(r) => r,
        Err(e) if e.is_connect() => {
            println!("  cannot connect: {:#}", anyhow::Error::from(e));
            if target == Target::Local {
                println!(
                    "  The local llama-server is not running. Start it with:\n    {LOCAL_SERVE_HINT}"
                );
            }
            return Ok(ExitCode::FAILURE);
        }
        Err(e) => return Err(e).context("GET /health"),
    };
    let status = health.status();
    field(
        "GET /health",
        format!("{status} in {}", ms(started.elapsed())),
    );
    // Show short bodies like llama.cpp's {"status":"ok"}; not Google's full-page HTML errors.
    let body = health.text().unwrap_or_default();
    let body = body.trim();
    if !body.is_empty() {
        if body.len() <= 200 && !body.contains('\n') {
            field("body", body);
        } else {
            field("body", format!("({} bytes, not shown)", body.len()));
        }
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        println!(
            "  The server rejected the request's credentials. Cloud Run needs an identity token from an \
             account with roles/run.invoker (`gcloud auth print-identity-token`)."
        );
        return Ok(ExitCode::FAILURE);
    }
    if !status.is_success() {
        bail!("server is not healthy: GET /health returned {status}");
    }

    section("Model");
    let models: Value = authed(client.get(format!("{base}/v1/models")))
        .send()
        .and_then(|r| r.error_for_status())
        .context("GET /v1/models")?
        .json()
        .context("GET /v1/models returned something other than JSON")?;
    let served = models["data"].as_array().cloned().unwrap_or_default();
    for m in &served {
        let id = m["id"].as_str().unwrap_or("?");
        // vLLM reports max_model_len; llama.cpp reports meta.n_ctx.
        match m["max_model_len"]
            .as_u64()
            .or_else(|| m["meta"]["n_ctx"].as_u64())
        {
            Some(ctx) => field("served", format!("{id} (context {ctx} tokens)")),
            None => field("served", id),
        }
    }
    let model = match &args.model {
        Some(m) => {
            field("using", format!("{m} (from --model/GEMMA_MODEL)"));
            m.clone()
        }
        None => {
            let m = served
                .first()
                .and_then(|m| m["id"].as_str())
                .context("the server listed no models at /v1/models; pass --model")?
                .to_string();
            field("using", format!("{m} (first model the server lists)"));
            m
        }
    };

    section("Request");
    let chat_url = format!("{base}/v1/chat/completions");
    field("POST", &chat_url);
    let mut messages = Vec::new();
    if let Some(system) = &args.system {
        field("system", system);
        messages.push(json!({"role": "system", "content": system}));
    }
    field("prompt", &args.prompt);
    messages.push(json!({"role": "user", "content": args.prompt}));
    field("max_tokens", args.max_tokens);
    if args.max_tokens < 512 {
        println!(
            "  warning: below 512, Gemma 4 may still be reasoning when it hits the limit and return an empty answer"
        );
    }
    let mut request = json!({"model": model, "messages": messages, "max_tokens": args.max_tokens});
    if let Some(t) = args.temperature {
        field("temperature", t);
        request["temperature"] = json!(t);
    }

    let started = Instant::now();
    let response = authed(client.post(&chat_url).json(&request))
        .send()
        .context("POST /v1/chat/completions")?;
    let status = response.status();
    let text = response.text().context("reading the completion response")?;
    let latency = started.elapsed();
    if !status.is_success() {
        section("Error");
        field("status", status);
        println!("{}", indent(&text));
        return Ok(ExitCode::FAILURE);
    }
    let raw: Value = serde_json::from_str(&text).context("the completion response is not JSON")?;
    let parsed: ChatResponse =
        serde_json::from_value(raw.clone()).context("unexpected completion response shape")?;
    let choice = parsed
        .choices
        .first()
        .context("the response has no choices")?;
    let content = choice.message.content.as_deref().unwrap_or("").trim();
    let reasoning = [&choice.message.reasoning_content, &choice.message.reasoning]
        .into_iter()
        .flatten()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("");

    section("Answer");
    if !content.is_empty() {
        println!("{}", indent(content));
    } else if !reasoning.is_empty() {
        println!(
            "  (empty) The model was still reasoning when it stopped. This is Gemma 4 thinking, not a \
             broken server: raise --max-tokens."
        );
    } else {
        println!("  (empty)");
    }

    section("Reasoning");
    if reasoning.is_empty() {
        println!("  (none returned)");
    } else {
        field("length", format!("{} chars", reasoning.chars().count()));
        println!("{}", indent(reasoning));
    }

    section("Stats");
    let finish_reason = choice.finish_reason.as_deref().unwrap_or("?");
    field("finish_reason", finish_reason);
    if finish_reason == "length" {
        println!("  The reply hit max_tokens and is cut off.");
    }
    let completion_tokens = parsed.usage.as_ref().and_then(|u| u.completion_tokens);
    if let Some(usage) = &parsed.usage {
        field("prompt_tokens", opt(usage.prompt_tokens));
        field(
            "cached_tokens",
            opt(usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)),
        );
        field("completion_tokens", opt(usage.completion_tokens));
        field("total_tokens", opt(usage.total_tokens));
    }
    field("latency (client)", ms(latency));
    if let Some(n) = completion_tokens {
        field(
            "tokens/s (client)",
            format!(
                "{:.1}  (completion tokens / latency; includes network and prefill)",
                n as f64 / latency.as_secs_f64()
            ),
        );
    }
    print_object("Server timings (llama.cpp)", parsed.timings.as_ref());
    print_object("Server metrics (vLLM)", parsed.metrics.as_ref());

    section("Response");
    field("id", opt(parsed.id));
    field("model", opt(parsed.model));
    field("system_fingerprint", opt(parsed.system_fingerprint));

    if args.raw {
        section("Raw JSON");
        println!("{}", serde_json::to_string_pretty(&raw)?);
    }

    Ok(if content.is_empty() {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}

/// The bearer token to send, with where it came from; never printed.
fn resolve_token(args: &Args, target: Target) -> Result<Option<(&'static str, String)>> {
    if args.auth == Auth::None {
        return Ok(None);
    }
    if let Some(t) = &args.token {
        return Ok(Some(("--token/GEMMA_TOKEN", t.clone())));
    }
    if args.auth == Auth::Auto && target != Target::CloudRun {
        return Ok(None);
    }
    let out = Command::new("gcloud")
        .args(["auth", "print-identity-token"])
        .output()
        .context(
            "running `gcloud auth print-identity-token` (is the Google Cloud SDK installed?)",
        )?;
    if !out.status.success() {
        bail!(
            "`gcloud auth print-identity-token` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let token = String::from_utf8(out.stdout)?.trim().to_string();
    Ok(Some(("gcloud auth print-identity-token", token)))
}

fn print_object(title: &str, value: Option<&Value>) {
    let Some(Value::Object(map)) = value else {
        return;
    };
    if map.is_empty() {
        return;
    }
    section(title);
    for (key, v) in map {
        match v {
            Value::Number(n) if n.is_f64() => {
                field(key, format!("{:.2}", n.as_f64().unwrap_or_default()))
            }
            Value::String(s) => field(key, s),
            other => field(key, other),
        }
    }
}

fn section(title: &str) {
    println!(
        "\n== {title} {}",
        "=".repeat(66usize.saturating_sub(title.len()))
    );
}

fn field(key: &str, value: impl Display) {
    println!("  {key:<22} {value}");
}

fn opt(value: Option<impl Display>) -> String {
    value.map_or_else(|| "-".to_string(), |v| v.to_string())
}

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1000.0)
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
