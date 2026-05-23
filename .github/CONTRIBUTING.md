# Contributing to feedy

Thanks for contributing.

## Ground rules

Be respectful and constructive.
Keep changes focused and easy to review.
Discuss larger changes in an issue first.

## Development setup

1. Fork and clone the repository.
2. Install stable Rust.
3. Run:

```bash
cargo check
cargo test
```

## Coding standards

Use Rustfmt defaults.
Prefer clear and direct code over clever code.
Write tests for behavior changes.
Keep public behavior documented in README when relevant.

## Commit and PR guidance

Use meaningful commit messages.
Open one logical change per PR.
Fill out the PR template.
Link related issues.

## Before submitting

Run:

```bash
cargo fmt
cargo check
cargo test
```

If you change TUI behavior or CLI flags, update docs in README.

## Review expectations

Maintainers may ask for changes before merge.
PRs that break tests or introduce regressions will not be merged.
