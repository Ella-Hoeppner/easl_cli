# Easl CLI

CLI for [easl](https://github.com/Ella-Hoeppner/easl), the Enhanced Abstraction Shader Language.

## Installation

```cargo install --path .```

After running this, you can test that the installation was successful by running `easl run ./examples/raymarch.easl` from the root of this project. You should see a window open displaying a rotating, distorted cube shape.

`easl compile --web` needs the wasm32 target, since installing builds easl's web runtimes: `rustup target add wasm32-unknown-unknown`. To install without it, use `cargo install --path . --no-default-features --features interpreter`.

## Usage

### Commands

**compile** - Compile .easl files to .wgsl
- `easl compile <INPUT>` - Compile a single file or directory
- `--output, -o <OUTPUT>` - Specify output file or directory (defaults to input with .wgsl extension)
- `--watch, -w` - Watch for file changes and automatically recompile
- `--web` - Compile a program to a web page that runs it in the browser through WebGPU. The output is a directory (defaulting to the input's name with a `_web` suffix) holding `index.html`, which shows the program on a full-window canvas, `easl-program.js`, which exports `startEaslProgram(canvas)` for use in other pages, the compiled program (`easl-program.bin`), and the runtime that runs it (about 1 MB of WebAssembly, holding no compiler). Runtime errors give their message, but not where in the program they happened. Serve the directory over HTTP to view it. A program with atomics shared between its main and audio threads needs the page to be cross-origin isolated: serve it with the headers `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. Audio, MIDI, and file-loading builtins aren't supported on the web yet
- `--with-compiler` - With `--web`, ship the program's sources and a runtime that compiles them in the browser (about 3.3 MB of WebAssembly), instead of the compiled program: for pages that change the program as it runs, like a playground. Runtime errors say where in the sources they happened

**check** - Typecheck .easl files without compiling
- `easl check <INPUT>` - Check a single file or directory

**format** - Format .easl files
- `easl format <INPUT>` - Format a single file or directory
- `--output, -o <OUTPUT>` - Specify output file or directory (defaults to formatting in-place)

**run** - Run a .easl shader as a standalone application
- `easl run <INPUT>` - Run a single .easl file in a window (the file must have a `@cpu` entry point for this to work)
- `--watch, -w` - Watch for file changes and hot-reload the shader

### Examples

```bash
# Compile a single file
easl compile shader.easl

# Compile all .easl files in a directory
easl compile ./shaders

# Compile with custom output location
easl compile ./src --output ./build

# Watch and recompile on changes
easl compile shader.easl --watch

# Compile a program to a web page, then serve it
easl compile program.easl --web
python3 -m http.server --directory program_web

# Run a shader with live preview
easl run examples/raymarch.easl

# Run with hot-reload
easl run shader.easl --watch
```
