# Makefile for gemma-rust

# Variables
BIN := gemma-rust
PROMPT ?= Why is the sky blue? Answer in two sentences.
LOCAL_RIG := $(HOME)/gemma4-dev/local-llamacpp-1650ti-2b-q4_0
CLOUD_RIG := $(HOME)/gemma4-dev/gpu-2B-cloudrun-devops-agent

.PHONY: all build debug prod release run run-debug run-cloud chat chat-cloud clean lint clippy fmt format fmt-check check test ci help

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
