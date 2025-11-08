# Easl CLI

CLI for [easl](https://github.com/Ella-Hoeppner/easl), the Enhanced Abstraction Shader Language.

## Installation

After cloning this repo, run `install.sh`. This will create an executable at `./bin/easl`. You can test that the executable was created successfully with `./bin/easl run ./examples/raymarch.easl`. You should see a window open displaying a rotating, distorted cube shape.

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
- `easl run <INPUT>` - Run a single .easl file in a window
- `--fragment, -f <NAME>` - Specify fragment entry point (auto-detected if only one exists)
- `--vertex, -v <NAME>` - Specify vertex entry point (auto-detected if only one exists)
- `--triangles, -t <COUNT>` - Specify number of triangles to render (can be defined in shader e.g `(def triangles: u32 5)`)
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
