# Contributing to aw-watcher-network-rs

Thank you for your interest in contributing to aw-watcher-network-rs! This document provides guidelines and instructions for contributing to this project.

## Development Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable, 1.70.0 or newer recommended)
- [Git](https://git-scm.com/downloads)
- For pre-commit hooks: [pre-commit](https://pre-commit.com/#install)

### Setting Up Your Development Environment

1. **Clone the repository**

   ```bash
   git clone https://github.com/ActivityWatch/aw-watcher-network-rs.git
   cd aw-watcher-network-rs
   ```

2. **Install pre-commit hooks**

   We use pre-commit hooks to ensure code quality. Install them with:

   ```bash
   pip install pre-commit
   pre-commit install
   ```

3. **Build the project**

   ```bash
   cargo build
   ```

4. **Run tests**

   ```bash
   cargo test
   ```

## Development Workflow

### Code Style

This project follows the standard Rust code style enforced by `rustfmt` and `clippy`. The pre-commit hooks will automatically check for style issues, but you can also run these manually:

```bash
cargo fmt --all
cargo clippy -- -D warnings
```

### Making Changes

1. Create a new branch for your changes:

   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make your changes and commit them. The pre-commit hooks will automatically run to check for issues.

3. Push your branch to GitHub:

   ```bash
   git push -u origin feature/your-feature-name
   ```

4. Create a Pull Request on GitHub.

### Pull Request Process

1. Ensure your code passes all CI checks.
2. Update the README.md if necessary with details of changes to the interface.
3. Your PR needs to be approved by at least one maintainer before it can be merged.
4. You may merge the PR once it has approval, or ask a maintainer to merge it for you.

## Testing

### Running Tests

Run the standard test suite with:

```bash
cargo test
```

### Platform-Specific Testing

This application runs on multiple platforms. If you're making changes that might affect platform-specific code, please test on those platforms if possible:

- For macOS-specific code: Test on macOS
- For Linux-specific code: Test on Linux
- For Windows-specific code: Test on Windows

If you don't have access to a specific platform, make sure to mention this in your PR so that maintainers can help with testing.



## Release Process

See the [README.md](README.md) for information about the release process.

## Reporting Bugs

When reporting bugs, please include:

1. Your operating system name and version
2. Any details about your local setup that might be helpful in troubleshooting
3. Detailed steps to reproduce the bug
4. What you expected to happen
5. What actually happened

## Feature Requests

Feature requests are welcome. Please provide:

1. A clear and detailed explanation of the feature you want
2. The motivation for the feature
3. Any potential implementation details or ideas you have

## Questions?

If you have any questions or need help, please open an issue on GitHub.

Thank you for contributing to aw-watcher-network-rs!
