# Contributing to Octopus API Gateway

Thank you for your interest in contributing to Octopus! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Pull Request Process](#pull-request-process)
- [Community](#community)

---

## Code of Conduct

This project adheres to a code of conduct that all contributors are expected to follow:

- Be respectful and inclusive
- Welcome newcomers and help them get started
- Focus on constructive feedback
- Assume good intentions

---

## Getting Started

### Prerequisites

- Rust 1.85 or later
- Git
- A GitHub account

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork locally:

```bash
git clone https://github.com/YOUR_USERNAME/octopus.git
cd octopus
```

3. Add the upstream repository:

```bash
git remote add upstream https://github.com/xraph/octopus.git
```

### Build and Test

```bash
# Build the project
cargo build --all-features

# Run tests
cargo test --all-features

# Run checks
./scripts/dev.sh check
```

---

## Development Workflow

### 1. Create a Branch

Create a branch for your work:

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b fix/your-bug-fix
```

Branch naming conventions:
- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation changes
- `refactor/` - Code refactoring
- `test/` - Test improvements

### 2. Make Changes

- Write clean, idiomatic Rust code
- Follow the existing code style
- Add tests for new functionality
- Update documentation as needed

### 3. Commit Changes

Write clear, descriptive commit messages:

```bash
git commit -m "feat: add WebSocket keep-alive support"
git commit -m "fix: resolve memory leak in connection pool"
git commit -m "docs: update FARP integration guide"
```

Commit message format:
- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation changes
- `test:` - Test changes
- `refactor:` - Code refactoring
- `perf:` - Performance improvements
- `chore:` - Build/tooling changes

### 4. Keep Your Branch Updated

```bash
git fetch upstream
git rebase upstream/main
```

---

## Coding Standards

### Rust Style

Follow the Rust style guide and use `rustfmt`:

```bash
cargo fmt --all
```

### Linting

Use Clippy and address all warnings:

```bash
cargo clippy --all-features -- -D warnings
```

### Code Organization

- Keep functions small and focused (< 50 lines)
- Use meaningful variable and function names
- Add comments for complex logic
- Organize code into logical modules

### Error Handling

- Use the `Result` type for fallible operations
- Provide context with error messages
- Use `thiserror` for custom error types
- Never use `unwrap()` in library code

### Documentation

- Add doc comments for public APIs
- Include examples in doc comments
- Update relevant documentation files

Example:

```rust
/// Validates a JWT token and returns the claims.
///
/// # Arguments
///
/// * `token` - The JWT token string to validate
///
/// # Returns
///
/// Returns `Ok(Claims)` if valid, `Err(Error)` otherwise.
///
/// # Examples
///
/// ```
/// let token = "eyJhbGc...";
/// let claims = validate_token(token)?;
/// assert_eq!(claims.sub, "user-123");
/// ```
pub fn validate_token(token: &str) -> Result<Claims> {
    // Implementation
}
```

---

## Testing

### Unit Tests

Add unit tests for all new functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_name() {
        // Arrange
        let input = create_test_input();
        
        // Act
        let result = function_under_test(input);
        
        // Assert
        assert_eq!(result, expected_value);
    }

    #[tokio::test]
    async fn test_async_feature() {
        // Test async code
    }
}
```

### Integration Tests

Place integration tests in `tests/` directory:

```bash
tests/
  integration_test.rs
```

### Running Tests

```bash
# Run all tests
cargo test --all-features

# Run specific crate tests
cargo test -p octopus-core

# Run with output
cargo test --all-features -- --nocapture

# Run with coverage
cargo tarpaulin --all-features
```

### Test Coverage

- Aim for 80%+ coverage for new code
- Focus on critical paths and edge cases
- Include error cases in tests

---

## Documentation

### Types of Documentation

1. **Code Comments**
   - Explain "why", not "what"
   - Document complex algorithms
   - Add examples for public APIs

2. **Design Documents**
   - Update `design/` directory for architectural changes
   - Include diagrams where helpful
   - Document trade-offs and decisions

3. **User Documentation**
   - Update README.md
   - Update QUICKSTART.md
   - Add examples to `examples/`

### Building Documentation

```bash
# Build and open documentation
cargo doc --all-features --no-deps --open
```

---

## Pull Request Process

### Before Submitting

1. **Run all checks:**
   ```bash
   ./scripts/dev.sh check
   ```

2. **Update documentation:**
   - Update relevant .md files
   - Add doc comments to new code
   - Update CHANGELOG.md

3. **Test thoroughly:**
   - Run all tests
   - Test manually if applicable
   - Check for edge cases

### Submitting a PR

1. **Push your branch:**
   ```bash
   git push origin feature/your-feature-name
   ```

2. **Create a pull request on GitHub**

3. **Fill out the PR template:**
   - Describe the changes
   - Link related issues
   - Add screenshots if UI changes
   - List breaking changes if any

4. **Checklist:**
   - [ ] Tests pass
   - [ ] Code is formatted (`cargo fmt`)
   - [ ] No Clippy warnings
   - [ ] Documentation updated
   - [ ] CHANGELOG.md updated

### PR Review Process

- Maintainers will review your PR
- Address feedback and requested changes
- Keep the PR focused and atomic
- Squash commits if requested

### After Merge

- Delete your branch
- Pull the latest main branch
- Celebrate! 🎉

---

## Community

### Getting Help

- **Documentation**: Start with `docs/` and `design/` directories
- **Examples**: Check `examples/` for usage examples
- **Issues**: Search existing issues before creating new ones

### Reporting Bugs

When reporting bugs, include:

1. **Description**: Clear description of the issue
2. **Steps to Reproduce**: Minimal steps to reproduce
3. **Expected Behavior**: What should happen
4. **Actual Behavior**: What actually happens
5. **Environment**: OS, Rust version, Octopus version
6. **Logs**: Relevant error messages or logs

### Requesting Features

When requesting features, include:

1. **Use Case**: What problem does it solve?
2. **Proposed Solution**: How should it work?
3. **Alternatives**: Have you considered alternatives?
4. **Additional Context**: Any other relevant information

---

## Crate-Specific Guidelines

### octopus-core

- Keep the API minimal and stable
- Changes here affect all other crates
- Ensure backward compatibility

### octopus-plugins

- Follow the plugin API specification
- Add examples for new plugin types
- Document plugin lifecycle

### octopus-admin

- Keep the UI simple and fast
- Use Alpine.js patterns
- Ensure mobile responsiveness

---

## License

By contributing to Octopus, you agree that your contributions will be licensed under both:

- MIT License
- Apache License 2.0

---

## Questions?

If you have questions not covered here:

- Check the [AGENT_GUIDE.md](docs/AGENT_GUIDE.md)
- Open a discussion on GitHub
- Reach out to maintainers

---

**Thank you for contributing to Octopus! 🐙**


