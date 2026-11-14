# Easl CLI

CLI for [easl](https://github.com/Ella-Hoeppner/easl), the Enhanced Abstraction Shader Language.

## Installation

```
cargo install --locked easl_cli
```

This installs the `easl` command, and needs Rust 1.90 or newer. On Linux, building it needs the development packages of the audio and windowing libraries (on Debian and Ubuntu, `libasound2-dev`, plus the usual X11 or Wayland ones). Programs that load video need [ffmpeg](https://ffmpeg.org/download.html) on the PATH when they run.

To check that it works, clone this repository and run `easl run examples/raymarch.easl` from its root. You should see a window open displaying a rotating, distorted cube shape.

## Usage

### Commands

**compile** - Compile .easl files to .wgsl
- `easl compile <INPUT>` - Compile a single file or directory
- `--output, -o <OUTPUT>` - Specify output file or directory (defaults to input with .wgsl extension)
- `--watch, -w` - Watch for file changes and automatically recompile
- `--web` - Compile a program to a web page that runs it in the browser through WebGPU. The output is a directory (defaulting to the input's name with a `_web` suffix) holding `index.html`, which shows the program on a full-window canvas, `easl-program.js`, which exports `startEaslProgram(canvas)` for use in other pages, the compiled program (`easl-program.bin`), and the runtime that runs it (about 700 KB of WebAssembly, holding no compiler). Runtime errors give their message, but not where in the program they happened. A program with several `@cpu` entry points runs the one the page names: `startEaslProgram(canvas, { entry: "name" })`. Serve the directory over HTTP to view it. A program with atomics shared between its main and audio threads needs the page to be cross-origin isolated: serve it with the headers `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. File-loading builtins, MIDI output, audio input, and `@external` variables aren't supported on the web yet
- `--with-compiler` - With `--web`, ship the program's sources and a runtime that compiles them in the browser (about 2.9 MB of WebAssembly), instead of the compiled program: for pages that change the program as it runs, like a playground. Runtime errors say where in the sources they happened

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
