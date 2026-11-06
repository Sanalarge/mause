# Contributing to Mause

Thank you for helping improve Mause.

## Branch model

`main` is the only permanent branch and should always remain releasable. Do not create long-lived `develop` or `release` branches.

Create a short-lived branch from the latest `main`:

- `feature/short-name` for a new capability
- `fix/short-name` for a defect
- `docs/short-name` for documentation
- `chore/short-name` for maintenance, build or CI work

Keep each branch focused on one change. Delete it after the pull request is squash-merged.

## Development flow

1. Open an issue for substantial changes so the approach can be discussed.
2. Update your local `main` and create a topic branch.
3. Make a focused change without adding comments to source code.
4. Run the relevant checks locally.
5. Open a pull request and complete the template.
6. Resolve review feedback and wait for CI to pass.
7. Squash-merge the pull request into `main`.

## Commit messages

Use an imperative Conventional Commit prefix:

- `feat:` new behavior
- `fix:` defect correction
- `docs:` documentation only
- `build:` build or packaging changes
- `ci:` workflow changes
- `refactor:` internal restructuring without behavior changes
- `test:` test changes
- `chore:` repository maintenance

## Local checks

```console
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D clippy::correctness
```

Platform-specific changes should also build on the affected platform. Android changes should pass `./gradlew assembleDebug`; Windows installer changes should pass `packaging/windows/Test-Installer.ps1` against a newly generated setup.

## Pull requests

Explain the problem, the solution, how you tested it and any platform-specific impact. Include screenshots or a short recording for visible interface changes. Do not mix unrelated cleanup into the same pull request.

By contributing, you agree that your contribution is licensed under the Apache License 2.0.

