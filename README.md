````markdown
# haxlint (VS Code Extension)

A VS Code extension that provides Rust linting to look for use of as-yet unsupported core models, which can't be extracted by Hax.

Runs as a Language Server Protocol (LSP) server and reports unsupported Rust APIs as VS Code diagnostics (squiggly underlines and entries in the Problems panel).

## Installation (prebuilt)

Install prebuilt .vsix, build from source, or build debug version.

## Build debug extension version

### Requirements

- Rust toolchain
- Node.js and npm
- VS Code

```bash
npm install -g @vscode/vsce
```

Clone this project and install dependencies with `cargo install`. Then:

```bash
cargo build --debug
```

The binary will be created at

```text
target/debug/haxlint
```

Copy the binary into the extension directory:

```bash
cp target/debug/haxlint path/to/vscode-extensions/haxlint-debug # ie ~/.vscode/extensions/haxlint-debug on your machine
```

The extension directory should contain:

```text
haxlint-vscode/
├── package.json
├── extension.js
├── haxlint
└── coverage.json
```

`coverage.json` contains the list of (currently) unsupported core model APIs.

### Debug extension

1. Open the `haxlint-vscode` folder in VS Code.
2. Open **Run and Debug**.
3. Select **Run haxlint extension**.
4. Choose **Run → Start Debugging**.

A new Extension Development Host window opens.

Open a Rust project in that window and edit a `.rs` file. Diagnostics will appear in the editor (underlines), or in  **View → Problems**. Debug output for the extension is visible in

```
View → Output → Extension Host
```

## Build VSIX Package

```bash
cargo build --release

cp target/release/haxlint .
```

Package the extension:

```bash
vsce package
```
In VSCode, **Extensions** > **Install from VSIX...** or 

```bash
code --install-extension haxlint-x.y.z.vsix
```

Haxlint will start automatically when a Rust project is opened.

## Troubleshooting

### Extension starts but no warnings appear

Check:

```
View → Output → Extension Host
```

You should see startup and lint messages from haxlint.

### Binary not found

Ensure the executable exists in the extension directory and is executable:

```bash
chmod +x haxlint
```
