# Makefile for gemma-rust

# Variables
BIN := gemma-rust
PROMPT ?= Why is the sky blue? Answer in two sentences.
LOCAL_RIG := $(HOME)/gemma4-dev/local-llamacpp-1650ti-2b-q4_0
CLOUD_RIG := $(HOME)/gemma4-dev/gpu-2B-cloudrun-devops-agent

.PHONY: all build debug prod release run run-debug run-cloud chat chat-cloud status status-cloud clean lint clippy fmt format fmt-check check test ci help

# The default target
all: debug

# Build the project for development
debug:
	@echo "Building debug..."
	@cargo build

build: debug

# Build the project for release
prod:
	@echo "Building release..."
	@cargo build --release
	@echo "Binary: target/release/$(BIN)"

release: prod

# Ask the local llama.cpp rig (release build)
run:
	@echo "Asking the local rig..."
	@cargo run --release -- "$(PROMPT)"

# Ask the local llama.cpp rig (debug build)
run-debug:
	@echo "Asking the local rig (debug build)..."
	@cargo run -- "$(PROMPT)"

# Ask the Cloud Run vLLM service; endpoint comes from the rig, never hard-coded
run-cloud:
	@echo "Asking Cloud Run (first request may take minutes on a cold start)..."
	@GEMMA_ENDPOINT=$$(make -s -C $(CLOUD_RIG) endpoint) cargo run --release -- "$(PROMPT)"

# Interactive session with the local rig
chat:
	@cargo run --release -- --interactive

# Interactive session with Cloud Run
chat-cloud:
	@GEMMA_ENDPOINT=$$(make -s -C $(CLOUD_RIG) endpoint) cargo run --release -- --interactive

# Local rig: host GPU memory, then what the server reports (version, model, slots, metrics)
status:
	@printf '\n== Host GPU (nvidia-smi) =============================================\n'
	@if command -v nvidia-smi >/dev/null; then \
		nvidia-smi --query-gpu=name,driver_version,memory.total,memory.used,memory.free,utilization.gpu,temperature.gpu --format=csv | sed 's/^/  /'; \
		nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv | sed 's/^/  /'; \
	else echo "  nvidia-smi not found"; fi
	@cargo run -q --release -- --status

# Cloud Run: the deployment (gcloud, read-only), then what the server reports. The service,
# project and region are read from the rig's Makefile, never written here.
status-cloud:
	@rv() { $(MAKE) -s --no-print-directory -C $(CLOUD_RIG) --eval='print-%: ; @echo $$($$*)' print-$$1; }; \
	printf '\n== Deployment (gcloud run services describe) =========================\n'; \
	gcloud run services describe "$$(rv SERVICE_NAME)" --project="$$(rv PROJECT_ID)" \
		--region="$$(rv REGION)" --format=json | jq -r "$$DEPLOY_JQ"
	@GEMMA_ENDPOINT=$$(make -s -C $(CLOUD_RIG) endpoint) cargo run -q --release -- --status

# Rows for status-cloud, in the CLI's "  key<22> value" layout
define DEPLOY_JQ
def row(k; v): "  " + ((k + "                        ")[:23]) + (if v == null then "-" else (v | tostring) end);
row("service"; .metadata.name),
row("url"; .status.url),
row("ready"; [.status.conditions[] | select(.type == "Ready") | .status + (if .message then " (" + .message + ")" else "" end)] | first),
row("revision"; .status.latestReadyRevisionName),
row("traffic"; [.status.traffic[] | "\(.percent)% \(.revisionName // "latest")"] | join(", ")),
row("scaling"; if .metadata.annotations["run.googleapis.com/scalingMode"] == "manual"
  then "manual: \(.metadata.annotations["run.googleapis.com/manualInstanceCount"]) instance(s) always running, billed while idle"
  else "auto: \(.spec.template.metadata.annotations["autoscaling.knative.dev/minScale"] // "0") to \(.spec.template.metadata.annotations["autoscaling.knative.dev/maxScale"] // "?") instances" end),
row("gpu"; "\(.spec.template.spec.containers[0].resources.limits["nvidia.com/gpu"] // "0") x \(.spec.template.spec.nodeSelector["run.googleapis.com/accelerator"] // "none")"),
row("cpu"; .spec.template.spec.containers[0].resources.limits.cpu),
row("memory"; .spec.template.spec.containers[0].resources.limits.memory),
row("concurrency"; .spec.template.spec.containerConcurrency),
row("timeout"; "\(.spec.template.spec.timeoutSeconds) s"),
row("image"; .spec.template.spec.containers[0].image),
(.spec.template.spec.containers[0].args // [] | .[] | row("arg"; .))
endef
export DEPLOY_JQ

# Clean the project
clean:
	@echo "Cleaning the project..."
	@cargo clean

# Lint the code: clippy on every target, then the format check
lint:
	@echo "Linting code..."
	@cargo clippy --all-targets -- -D warnings
	@cargo fmt --all -- --check

clippy:
	@echo "Running clippy..."
	@cargo clippy --all-targets -- -D warnings

# Format the code
fmt:
	@echo "Formatting code..."
	@cargo fmt --all

format: fmt

# Check formatting without changing files
fmt-check:
	@echo "Checking formatting..."
	@cargo fmt --all -- --check

# Check the code
check:
	@echo "Checking the code..."
	@cargo check --all-targets

# Run tests
test:
	@echo "Running tests..."
	@cargo test

# Everything a CI run would do
ci: lint test prod

help:
	@echo "Makefile for gemma-rust"
	@echo ""
	@echo "Usage:"
	@echo "    make <target> [PROMPT=\"...\"]"
	@echo ""
	@echo "Targets:"
	@echo "    all          (default) same as 'debug'"
	@echo "    debug        Build for development (alias: build)"
	@echo "    prod         Build optimised release binary (alias: release)"
	@echo "    run          Ask the local llama.cpp rig (release build)"
	@echo "    run-debug    Ask the local llama.cpp rig (debug build)"
	@echo "    run-cloud    Ask the Cloud Run vLLM service (identity token via gcloud)"
	@echo "    chat         Interactive session with the local rig (keeps the conversation)"
	@echo "    chat-cloud   Interactive session with Cloud Run"
	@echo "    status       Local rig: GPU memory, server version, model, slots, metrics"
	@echo "    status-cloud Cloud Run: deployment (gcloud), server version, model, metrics"
	@echo "    clean        Remove build artefacts"
	@echo "    lint         clippy -D warnings + format check"
	@echo "    clippy       clippy only"
	@echo "    fmt          Format the code (alias: format)"
	@echo "    fmt-check    Check formatting without changing files"
	@echo "    check        cargo check"
	@echo "    test         Run tests"
	@echo "    ci           lint + test + prod"
	@echo ""
	@echo "Servers are started by the rigs, not here:"
	@echo "    make -C $(LOCAL_RIG) serve"
	@echo "    make -C $(CLOUD_RIG) status"
