# Contributing to RBAC-Gate

Thank you for your interest in contributing!

## Getting Started

1. Fork the repository
2. Clone your fork and create a feature branch
3. Install Rust stable 1.96+ via [rustup](https://rustup.rs)
4. Run `make all` to verify everything builds and passes

## Development Workflow

```console
make build       # compile
make test        # run all tests
make fmt         # format code
make lint        # clippy + fmt check
make doc         # rustdoc with warnings
make audit       # cargo audit
```

## Pull Requests

- Keep PRs focused on a single change
- Include tests for new functionality
- Run `make lint` and `make test` before submitting
- Write a clear PR description explaining *why* the change is needed

## Code Style

- Follow existing patterns in the codebase
- Use `cargo fmt` formatting (see `rustfmt.toml`)
- Address all `clippy` warnings (see `clippy.toml`)
- Avoid `unsafe` code

## Commit Messages

- Use conventional commit prefixes: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`
- Keep the subject line under 72 characters
- Explain the *why* in the body when the change is non-trivial

## Reporting Issues

- Use GitHub Issues for bug reports and feature requests
- For security vulnerabilities, see [SECURITY.md](SECURITY.md)
