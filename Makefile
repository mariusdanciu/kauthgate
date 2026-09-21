# -------------------------------------------------------------------
# Configuration
# -------------------------------------------------------------------

VERSION          ?= $(shell perl -ne 'print $$1 if /^version\s*=\s*"(.+)"/' Cargo.toml)
IMAGE            ?= rbac-gate
CONTAINER_ENGINE ?= $(shell command -v podman 2>/dev/null || command -v docker 2>/dev/null)
V                ?=

ifneq ($(V),)
  _NOCAPTURE := -- --nocapture
endif

.PHONY: all build release check clean \
	test lint fmt doc audit \
	require-container-engine \
	container container-run \
	setup-hooks help

# -------------------------------------------------------------------
# All
# -------------------------------------------------------------------

all: build fmt lint test audit

# -------------------------------------------------------------------
# Build
# -------------------------------------------------------------------

build:
	cargo build

release:
	cargo build --release

check:
	cargo check

clean:
	cargo clean

# -------------------------------------------------------------------
# Container
# -------------------------------------------------------------------

require-container-engine:
ifndef CONTAINER_ENGINE
	$(error No container engine found — install podman or docker)
endif

container: | require-container-engine
	$(CONTAINER_ENGINE) build -t $(IMAGE):$(VERSION) -f Containerfile .

container-run: | require-container-engine
	$(CONTAINER_ENGINE) run --rm --network=host $(IMAGE):$(VERSION) 2>&1

# -------------------------------------------------------------------
# Test
# -------------------------------------------------------------------

test:
	cargo test $(_NOCAPTURE)

# -------------------------------------------------------------------
# Quality
# -------------------------------------------------------------------

lint:
	cargo clippy --all-targets -- -D warnings
	cargo fmt -- --check

fmt:
	cargo fmt

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

audit:
	cargo audit

# -------------------------------------------------------------------
# Dev Setup
# -------------------------------------------------------------------

setup-hooks:
	@mkdir -p .hooks
	ln -sf ../../.hooks/pre-commit .git/hooks/pre-commit
	@echo "Git hooks installed."

# -------------------------------------------------------------------
# Help
# -------------------------------------------------------------------

help:
	@echo "Variables:"
	@echo "  V=1                  show test output (--nocapture)"
	@echo ""
	@echo "Top-level:"
	@echo "  all                  build + fmt + lint + test + audit"
	@echo ""
	@echo "Build:"
	@echo "  build                cargo build"
	@echo "  release              cargo build --release"
	@echo "  check                cargo check"
	@echo "  clean                cargo clean"
	@echo ""
	@echo "Test:"
	@echo "  test                 run all tests"
	@echo ""
	@echo "Quality:"
	@echo "  lint                 clippy + rustfmt check"
	@echo "  fmt                  format all code"
	@echo "  doc                  rustdoc with warnings"
	@echo "  audit                cargo audit"
	@echo ""
	@echo "Container:"
	@echo "  container            build container image"
	@echo "  container-run        run container (host network)"
