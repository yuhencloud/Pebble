# Contributing to Pebble

Thank you for your interest in contributing to Pebble! This document provides guidelines and instructions to help you get started.

## Getting Started

1. Fork the repository on GitHub.
2. Clone your fork locally.
3. Create a new branch for your feature or bug fix:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

```bash
cd pebble-app
npm install
npm run tauri dev
```

This starts both the Vite dev server and the Tauri application in watch mode.

## Code Style

- **Frontend**: TypeScript strict mode, React functional components, hooks for state management.
- **Backend**: Follow Rust formatting via `cargo fmt`. Run `cargo clippy` before submitting.
- **CSS**: Use CSS variables for theming. Avoid hardcoded colors.

## Commit Messages

Write clear, concise commit messages in the present tense:

```
Add iTerm2 precise pane jumping via AppleScript
Fix ghost instance cleanup logic
Update panel styles for dark mode
```

## Pull Request Process

1. Ensure your code builds (`npm run tauri build`).
2. Update the `README.md` or relevant documentation if you add or change features.
3. Open a pull request against the `main` branch.
4. Describe what your PR does and why. Link related issues if applicable.

## Reporting Issues

When reporting bugs, please include:
- macOS version
- Pebble version (or commit hash)
- Steps to reproduce
- Expected and actual behavior
- Screenshots if applicable

## Questions?

Feel free to open a [Discussion](#) or [Issue](#) on GitHub.
