# Pre-Commit Configuration

This repository uses [pre-commit](https://pre-commit.com/) to automate code
quality checks and formatting for YAML and Rust files. The following hooks are
configured to ensure code consistency and quality.

## Prerequisites

- Ensure you have Python and `pip` installed.
- Install `pre-commit` by running:

  ```bash
  pip install pre-commit
  ```

- Make sure you have Rust and Cargo installed. You can install them from
  [rust-lang.org](https://www.rust-lang.org/).

## Setup

To set up the pre-commit hooks in your repository, run:

```bash
pre-commit install
```

This will install the hooks defined in `.pre-commit-config.yaml`.

## Hooks Overview

### YAML Linting

- **Hook ID**: `yamllint`
- **Description**: This hook checks YAML files for syntax errors and best
  practices.
- **Command**:
  ```bash
  yamllint -c .yamllint.yaml --strict
  ```

### Rust Clippy

- **Hook ID**: `clippy`
- **Description**: This hook runs Clippy, a linting tool for Rust, to catch
  common mistakes and improve your code.
- **Command**:
  ```bash
  bash -c 'export NIGHTLY_VERSION=nightly-2024-09-24 && cargo +${NIGHTLY_VERSION}
  clippy --all -- --deny warnings'
  ```

### Rust Formatting

- **Hook ID**: `fmt`
- **Description**: This hook formats Rust code according to the standard Rust
  style.
- **Command**:
  ```bash
  bash -c 'export NIGHTLY_VERSION=nightly-2024-09-24 && cargo +${NIGHTLY_VERSION}
  fmt --all -- --check'
  ```

### Xtask Public API

- **Hook ID**: `xtask-public-api`
- **Description**: This hook runs a custom command defined in `xtask` to
  check the public API of the Rust project.
- **Command**:
  ```bash
  bash -c 'export NIGHTLY_VERSION=nightly-2024-09-24 && cargo xtask public-api
  --toolchain ${NIGHTLY_VERSION}'
  ```

## Usage

To manually run all configured hooks, execute:

```bash
pre-commit run --all-files
```

You can also run a specific hook by specifying its ID. For example, to run
Clippy, use:

```bash
pre-commit run clippy --all-files
```

## Customizing the Toolchain

The hooks are configured to use a specific nightly version of Rust
(`nightly-2024-09-24`). If you want to change the nightly version, simply
update the `NIGHTLY_VERSION` variable in the respective hook commands in
`.pre-commit-config.yaml`.
