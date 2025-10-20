mod app;

use clap::{Parser, Subcommand};
use easl::{
  compile_easl_source_to_wgsl, format_easl_source, get_easl_program_info,
};
use hollow::sketch::Sketch;
use std::fs;
use std::path::PathBuf;

use crate::app::{RunConfig, UserSketch};

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
    /// Path of the .easl file to compile
    input: PathBuf,

    /// Output file path, defaults to input file with .wgsl extension
    #[arg(short, long)]
    output: Option<PathBuf>,
  },
  /// Typecheck a .easl file without comiling
  Check {
    /// Path of the .easl file to check
    input: PathBuf,
  },
  /// Format a .easl file
  Format {
    /// Path of the .easl file to format
    input: PathBuf,

    /// Output file path, defaults to same as input
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
      long_help = "Name of the fragment entry point.\n\
                   Must be a function marked as @fragment.\n\
                   May be omitted if file has only one fragment entry."
    )]
    fragment: Option<String>,

    #[arg(
      short,
      long,
      long_help = "Name of the vertex entry point.\n\
                   Must be a function marked as @vertex.\n\
                   May be omitted if file has only one vertex entry."
    )]
    vertex: Option<String>,

    #[arg(
      short,
      long,
      long_help = "The number of triangles to render.\n\
                   Must be a positive integer.\n\
                   May be omitted if specified in the file\n\
                   e.g. `(def triangles: u32 100)`"
    )]
    triangles: Option<u32>,
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

fn compile_file(input: PathBuf, output: Option<PathBuf>) -> Result<(), String> {
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

fn check_file(input: PathBuf) -> Result<(), String> {
  let easl_source = read_source(&input)?;
  println!("Typechecking {}...", input.display());
  match compile_easl_source_to_wgsl(&easl_source) {
    Ok(_) => {
      println!("Program typechecked successfully!");
    }
    Err(error_log) => {
      println!("{error_log:#?}");
    }
  }
  Ok(())
}

fn format_file(input: PathBuf, output: Option<PathBuf>) -> Result<(), String> {
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

fn run_file(
  input: PathBuf,
  fragment: Option<String>,
  vertex: Option<String>,
  triangles: Option<u32>,
) -> Result<(), String> {
  let easl_source = read_source(&input)?;
  println!("Running {}...", input.display());
  match try_compile_easl(&easl_source) {
    Ok(wgsl) => {
      let program_info = get_easl_program_info(&easl_source).unwrap().unwrap();
      let fragment_entry = if let Some(fragment) = fragment {
        if program_info.fragment_entries.contains(&fragment) {
          fragment
        } else {
          return Err(format!("No fragment entry point named '{fragment}'"));
        }
      } else {
        match program_info.fragment_entries.len() {
          0 => return Err(format!("No fragment entry point found")),
          1 => program_info.fragment_entries[0].clone(),
          _ => {
            return Err(format!(
              "Multiple fragment entry points found. Use '--fragment' to \
              specify one."
            ));
          }
        }
      };
      let vertex_entry = if let Some(vertex) = vertex {
        if program_info.vertex_entries.contains(&vertex) {
          vertex
        } else {
          return Err(format!("No vertex entry point named '{vertex}'"));
        }
      } else {
        match program_info.vertex_entries.len() {
          0 => return Err(format!("No vertex entry point found")),
          1 => program_info.vertex_entries[0].clone(),
          _ => {
            return Err(format!(
              "Multiple fragment entry points found. Use '--fragment' to \
              specify one."
            ));
          }
        }
      };
      let triangles = if let Some(triangles) = triangles {
        triangles
      } else {
        if let Some(triangles) =
          program_info.global_vars.iter().find_map(|var| {
            if var.uniform_info.is_none()
              && var.name == "triangles"
              && let Some(value) = &var.value
              && let Ok(triangles) = value.parse::<u32>()
            {
              Some(triangles)
            } else {
              None
            }
          })
        {
          triangles
        } else {
          return Err(format!(
            "No triangle count specified. Specify it with the `--triangles` \
              flag or by defining it in your source file, e.g. \
              '(def triangles: u32 5)'"
          ));
        }
      };
      UserSketch::new(
        wgsl,
        RunConfig {
          fragment_entry,
          vertex_entry,
          triangles,
        },
      )
      .run();
      Ok(())
    }
    Err(e) => Err(e),
  }
}

fn main() {
  let cli = Cli::parse();
  if let Err(e) = match cli.command {
    Command::Compile { input, output } => compile_file(input, output),
    Command::Check { input } => check_file(input),
    Command::Format { input, output } => format_file(input, output),
    Command::Run {
      input,
      fragment,
      vertex,
      triangles,
    } => run_file(input, fragment, vertex, triangles),
  } {
    eprintln!("{e}");
    std::process::exit(1);
  }
}
