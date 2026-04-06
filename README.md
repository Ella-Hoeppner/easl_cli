# Easl CLI

CLI for [easl](https://github.com/Ella-Hoeppner/easl), the Enhanced Abstraction Shader Language.

## Installation

```cargo install --path .```

After running this, you can test that the installation was successful by running `easl run ./examples/raymarch.easl` from the root of this project. You should see a window open displaying a rotating, distorted cube shape.

## Usage

### Commands

**compile** - Compile .easl files to .wgsl
- `easl compile <INPUT>` - Compile a single file or directory
- `--output, -o <OUTPUT>` - Specify output file or directory (defaults to input with .wgsl extension)
- `--watch, -w` - Watch for file changes and automatically recompile

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

# Run a shader with live preview
easl run examples/raymarch.easl

# Run with hot-reload
easl run shader.easl --watch
```
