# haxlint

Very basic VS Code extension that provides Rust linting to look for use of as-yet unsupported [core models](https://github.com/cryspen/rust-core-models) that can't be extracted by [Hax](https://github.com/cryspen/hax) to warn before beginning long-running verification commands.

## Installation

Install prebuilt .vsix, build from source, or build debug version and try with vscode extension debugger.

## Build from source

### Requirements

- Rust toolchain
- Node.js and npm
- VS Code

```bash
npm install -g @vscode/vsce
```

Clone this project and install dependencies with `cargo install`. Then `make debug` (Rust binary only) or `make release` (packaged extension).

### Debug extension

Clone this repo, `make debug`. Open VS Code.
**Run and Debug> Start Debugging**.

## Caveats: Not Fancy

This does not detect all unsupported use, unsupported patterns (such as fnmut closures), etc, and doesn't handle all edge cases. This code was partially AI-generated.
