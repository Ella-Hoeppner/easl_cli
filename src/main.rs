use clap::{Parser, Subcommand};
#[cfg(feature = "interpreter")]
use easl::compiler::builtins::built_in_macros;
#[cfg(feature = "interpreter")]
use easl::compiler::program::Program;
#[cfg(feature = "interpreter")]
use easl::interpreter::run_program_entry;
#[cfg(feature = "interpreter")]
use easl::parse::parse_easl_without_comments;
use easl::{compile_easl_source_to_wgsl, format_easl_source};
use notify::{
  Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

#[derive(Parser)]
#[command(name = "easl")]
#[command(about = "Easl compiler")]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Compile a .easl file to .wgsl
  Compile {
    /// Path of the .easl file or directory to compile
    input: PathBuf,

    /// Output file or directory, defaults to input file with .wgsl extension
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Watch for file changes and recompile automatically
    #[arg(short, long)]
    watch: bool,
  },
  /// Typecheck a .easl file without comiling
  Check {
    /// Path of the .easl file or directory to check
    input: PathBuf,
  },
  /// Format a .easl file
  Format {
    /// Path of the .easl file or directory to format
    input: PathBuf,

    /// Output file or directory, defaults to same as input
    #[arg(short, long)]
    output: Option<PathBuf>,
  },
  /// Run a .easl file as a standalone application
  Run {
    /// Path of the .easl file to run
    input: PathBuf,

    #[arg(
      short,
      long,
      long_help = "Name of the `@cpu` entry point.\n\
                   May be omitted if file has only one cpu entry."
    )]
    entry: Option<String>,

    /// Watch for file changes and hot-reload the shader
    #[arg(short, long)]
    watch: bool,
  },
}

fn read_source(input: &PathBuf) -> Result<String, String> {
  fs::read_to_string(&input).map_err(|e| {
    format!(
      "Error: Failed to read input file {}\n{}",
      input.display(),
      e
    )
  })
}

#[cfg(feature = "interpreter")]
fn try_get_validated_easl_program(
  easl_source: &str,
) -> Result<Program, String> {
  let document = parse_easl_without_comments(easl_source);
  let (mut program, errors) =
    Program::from_easl_document(&document, built_in_macros());
  if !errors.is_empty() {
    return Err(errors.describe(&document));
  }
  let errors = program.validate_raw_program();
  if !errors.is_empty() {
    return Err(errors.describe(&document));
  }
  Ok(program)
}

fn try_compile_easl(easl_source: &str) -> Result<String, String> {
  match compile_easl_source_to_wgsl(easl_source) {
    Ok(Ok(wgsl)) => Ok(wgsl),
    Ok(Err((document, errors))) => Err(format!(
      "Compilation failed due to errors:\n\n{}",
      errors.describe(&document)
    )),
    Err(mut failed_document) => {
      Err(format!("Compilation failed due to parsing error:\n\n{}", {
        let mut errors = vec![];
        std::mem::swap(&mut errors, &mut failed_document.parsing_failures);
        errors
          .into_iter()
          .map(|err| err.describe(&failed_document))
          .collect::<Vec<String>>()
          .join("\n\n")
      }))
    }
  }
}

fn find_easl_files(dir: &PathBuf) -> Result<Vec<PathBuf>, String> {
  let mut easl_files = Vec::new();

  let entries = fs::read_dir(dir).map_err(|e| {
    format!("Error: Failed to read directory {}\n{}", dir.display(), e)
  })?;

  for entry in entries {
    let entry = entry
      .map_err(|e| format!("Error: Failed to read directory entry\n{}", e))?;
    let path = entry.path();

    if path.is_dir() {
      // Recursively search subdirectories
      easl_files.extend(find_easl_files(&path)?);
    } else if path.extension().and_then(|s| s.to_str()) == Some("easl") {
      easl_files.push(path);
    }
  }

  Ok(easl_files)
}

fn compile_single_file(
  input: PathBuf,
  output: Option<PathBuf>,
) -> Result<(), String> {
  let easl_source = read_source(&input)?;

  println!("Compiling {}...", input.display());
  match try_compile_easl(&easl_source) {
    Ok(wgsl) => {
      let output_path = output.unwrap_or_else(|| {
        let mut output_path = input.clone();
        output_path.set_extension("wgsl");
        output_path
      });

      fs::write(&output_path, wgsl).map_err(|e| {
        format!(
          "Error: Failed to write output file {}\n{}",
          output_path.display(),
          e
        )
      })?;

      println!("Finished: {}", output_path.display());
      Ok(())
    }
    Err(e) => Err(e),
  }
}

fn get_output_path_for_file(
  file: &Path,
  input_base: &Path,
  output_base: &Option<PathBuf>,
) -> Result<PathBuf, String> {
  if let Some(output_dir) = output_base {
    if input_base.is_dir() {
      // Calculate relative path from input directory
      let relative_path = file.strip_prefix(input_base).map_err(|e| {
        format!(
          "Error: Failed to calculate relative path for {}\n{}",
          file.display(),
          e
        )
      })?;

      // Construct output path with same relative structure
      let mut out_path = output_dir.join(relative_path);
      out_path.set_extension("wgsl");

      // Create parent directories if they don't exist
      if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
          format!(
            "Error: Failed to create directory {}\n{}",
            parent.display(),
            e
          )
        })?;
      }

      Ok(out_path)
    } else {
      // Single file with output specified
      Ok(output_dir.clone())
    }
  } else {
    // No output specified, use default
    let mut output_path = file.to_path_buf();
    output_path.set_extension("wgsl");
    Ok(output_path)
  }
}

fn compile_file(
  input: PathBuf,
  output: Option<PathBuf>,
  watch: bool,
) -> Result<(), String> {
  if watch {
    // Initial compilation
    compile_once(&input, &output)?;

    // Build initial content cache
    let mut file_contents: HashMap<PathBuf, String> = HashMap::new();
    let files_to_watch = if input.is_dir() {
      find_easl_files(&input)?
    } else {
      vec![input.clone()]
    };

    for file in &files_to_watch {
      if let Ok(content) = fs::read_to_string(file) {
        file_contents.insert(file.clone(), content);
      }
    }

    // Set up file watcher
    println!("\nWatching for changes... (Press Ctrl+C to stop)");

    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
      .map_err(|e| format!("Error: Failed to create file watcher\n{}", e))?;

    // Watch the input path
    let watch_mode = if input.is_dir() {
      RecursiveMode::Recursive
    } else {
      RecursiveMode::NonRecursive
    };

    watcher.watch(&input, watch_mode).map_err(|e| {
      format!("Error: Failed to watch path {}\n{}", input.display(), e)
    })?;

    // Process file change events
    loop {
      match rx.recv() {
        Ok(Ok(Event {
          kind: EventKind::Modify(_),
          paths,
          ..
        })) => {
          for path in paths {
            if path.extension().and_then(|s| s.to_str()) == Some("easl") {
              // Read current file content
              let current_content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                  eprintln!("Error reading {}: {}", path.display(), e);
                  continue;
                }
              };

              // Check if content has actually changed
              if let Some(cached_content) = file_contents.get(&path) {
                if cached_content == &current_content {
                  // Content unchanged, skip recompilation
                  continue;
                }
              }

              println!("\n{} changed, recompiling...", path.display());
              let output_path =
                match get_output_path_for_file(&path, &input, &output) {
                  Ok(p) => Some(p),
                  Err(e) => {
                    eprintln!("{}", e);
                    continue;
                  }
                };

              if let Err(e) = compile_single_file(path.clone(), output_path) {
                eprintln!("{}", e);
              }

              // Update cached content after compilation attempt (success or failure)
              file_contents.insert(path.clone(), current_content);
            }
          }
        }
        Ok(Ok(_)) => {} // Ignore other event types
        Ok(Err(e)) => eprintln!("Watch error: {}", e),
        Err(e) => {
          return Err(format!("Error: Channel receive error\n{}", e));
        }
      }
    }
  } else {
    compile_once(&input, &output)
  }
}

fn compile_once(
  input: &PathBuf,
  output: &Option<PathBuf>,
) -> Result<(), String> {
  if input.is_dir() {
    // Compile all .easl files in the directory recursively
    let easl_files = find_easl_files(input)?;

    if easl_files.is_empty() {
      return Err(format!(
        "No .easl files found in directory {}",
        input.display()
      ));
    }

    println!(
      "Found {} .easl file(s) in {}",
      easl_files.len(),
      input.display()
    );

    let mut failed = Vec::new();
    for file in &easl_files {
      let output_path = match get_output_path_for_file(file, input, output) {
        Ok(p) => Some(p),
        Err(e) => {
          eprintln!("{}", e);
          failed.push(file);
          continue;
        }
      };

      if let Err(e) = compile_single_file(file.clone(), output_path) {
        eprintln!("{}", e);
        failed.push(file);
      }
    }

    if !failed.is_empty() {
      Err(format!("\nFailed to compile {} file(s)", failed.len()))
    } else {
      Ok(())
    }
  } else {
    // Compile single file
    let output_path = if output.is_some() {
      output.clone()
    } else {
      None
    };
    compile_single_file(input.clone(), output_path)
  }
}

fn check_single_file(input: PathBuf) -> Result<(), String> {
  let easl_source = read_source(&input)?;
  print!("Typechecking {}...   ", input.display());
  match try_compile_easl(&easl_source) {
    Ok(_) => {
      println!("✅");
      Ok(())
    }
    Err(error_description) => {
      println!("❌\n{error_description}\n");
      Err(error_description)
    }
  }
}

fn check_file(input: PathBuf) -> Result<(), String> {
  if input.is_dir() {
    // Check all .easl files in the directory recursively
    let easl_files = find_easl_files(&input)?;

    if easl_files.is_empty() {
      return Err(format!(
        "No .easl files found in directory {}",
        input.display()
      ));
    }

    println!(
      "Found {} .easl file(s) in {}",
      easl_files.len(),
      input.display()
    );

    let mut failed = Vec::new();
    for file in &easl_files {
      if let Err(_) = check_single_file(file.clone()) {
        failed.push(file);
      }
    }

    if !failed.is_empty() {
      Err(format!("\nFailed to typecheck {} file(s)", failed.len()))
    } else {
      Ok(())
    }
  } else {
    // Check single file
    check_single_file(input)
  }
}

fn format_single_file(
  input: PathBuf,
  output: Option<PathBuf>,
) -> Result<(), String> {
  let easl_source = read_source(&input)?;
  println!("Formatting {}...", input.display());
  let formatted = format_easl_source(&easl_source);
  let output_path = output.unwrap_or_else(|| input.clone());
  fs::write(&output_path, formatted).map_err(|e| {
    format!(
      "Error: Failed to write output file {}\n{}",
      output_path.display(),
      e
    )
  })?;
  println!("Formatted: {}", output_path.display());
  Ok(())
}

fn format_file(input: PathBuf, output: Option<PathBuf>) -> Result<(), String> {
  if input.is_dir() {
    // Format all .easl files in the directory recursively
    let easl_files = find_easl_files(&input)?;

    if easl_files.is_empty() {
      return Err(format!(
        "No .easl files found in directory {}",
        input.display()
      ));
    }

    println!(
      "Found {} .easl file(s) in {}",
      easl_files.len(),
      input.display()
    );

    let mut failed = Vec::new();
    for file in &easl_files {
      let output_path = if let Some(ref output_dir) = output {
        // Calculate relative path from input directory
        let relative_path = file.strip_prefix(&input).map_err(|e| {
          format!(
            "Error: Failed to calculate relative path for {}\n{}",
            file.display(),
            e
          )
        })?;

        // Construct output path with same relative structure
        let out_path = output_dir.join(relative_path);

        // Create parent directories if they don't exist
        if let Some(parent) = out_path.parent() {
          fs::create_dir_all(parent).map_err(|e| {
            format!(
              "Error: Failed to create directory {}\n{}",
              parent.display(),
              e
            )
          })?;
        }

        Some(out_path)
      } else {
        None
      };

      if let Err(e) = format_single_file(file.clone(), output_path) {
        eprintln!("{}", e);
        failed.push(file);
      }
    }

    if !failed.is_empty() {
      Err(format!("\nFailed to format {} file(s)", failed.len()))
    } else {
      Ok(())
    }
  } else {
    // Format single file
    format_single_file(input, output)
  }
}

#[cfg(feature = "interpreter")]
fn run_file(
  input: PathBuf,
  entry: Option<String>,
  watch: bool,
) -> Result<(), String> {
  let easl_source = read_source(&input)?;
  let program = try_get_validated_easl_program(&easl_source)?;
  if watch {
    todo!()
  } else {
    match run_program_entry(program, entry.as_ref().map(|s| s.as_str())) {
      Err(e) => return Err(format!("{e:?}")),
      _ => {}
    }
  }
  Ok(())
}

fn main() {
  unsafe {
    std::env::set_var("RUST_BACKTRACE", "1");
  }
  let cli = Cli::parse();
  if let Err(e) = match cli.command {
    Command::Compile {
      input,
      output,
      watch,
    } => compile_file(input, output, watch),
    Command::Check { input } => check_file(input),
    Command::Format { input, output } => format_file(input, output),
    Command::Run {
      input,
      entry,
      watch,
    } => {
      #[cfg(feature = "interpreter")]
      {
        run_file(input, entry, watch)
      }
      #[cfg(not(feature = "interpreter"))]
      {
        Err(
        "This build of the easl CLI was compiled without interpreter support. \
         Build the CLI with `--features interpreter` to enable the `run` \
         command."
          .to_string(),
      )
      }
    }
  } {
    eprintln!("{e}");
    std::process::exit(1);
  }
}
