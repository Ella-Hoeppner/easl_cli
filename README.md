# Easl CLI

CLI for [easl](https://github.com/Ella-Hoeppner/easl), the Enhanced Abstraction Shader Language.

## Installation

```cargo install --path .```

After running this, you can test that the installation was successful by running `easl run ./examples/raymarch.easl` from the root of this project. You should see a window open displaying a rotating, distorted cube shape.

`easl compile --web` needs the wasm32 target, since installing builds easl's web runtime: `rustup target add wasm32-unknown-unknown`. To install without it, use `cargo install --path . --no-default-features --features interpreter`.

## Usage

### Commands

**compile** - Compile .easl files to .wgsl
- `easl compile <INPUT>` - Compile a single file or directory
- `--output, -o <OUTPUT>` - Specify output file or directory (defaults to input with .wgsl extension)
- `--watch, -w` - Watch for file changes and automatically recompile
- `--web` - Compile a program to a web page that runs it in the browser through WebGPU. The output is a directory (defaulting to the input's name with a `_web` suffix) holding `index.html`, which shows the program on a full-window canvas, and `easl-program.js`, which exports `startEaslProgram(canvas)` for use in other pages. Serve the directory over HTTP to view it. Audio, MIDI, and file-loading builtins aren't supported on the web yet

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
