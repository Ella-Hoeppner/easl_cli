use clap::{Parser, Subcommand};
#[cfg(feature = "interpreter")]
use easl::CompilerTarget;
use easl::compiler::core::{
  compile_easl_file_to_wgsl, load_easl_program_from_file,
};
#[cfg(feature = "interpreter")]
use easl::compiler::entry::EntryPoint;
#[cfg(feature = "interpreter")]
use easl::compiler::program::Program;
use easl::format_easl_source;
#[cfg(feature = "interpreter")]
use easl::interpreter::{
  IOManager, StdoutIO, close_persistent_window, run_program_entry_from_path,
  run_program_entry_with_io_from_path,
};
use easl::parse::EaslMultiDocument;
#[cfg(feature = "web")]
use easl::web_bundle::{RUNTIME_SUPPORT_FILES, bundle_program};
use notify::{
  Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

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

    /// Compile to a web page that runs the program in the browser, through
    /// WebGPU. Output is a directory, defaulting to the input file's name
    /// with a `_web` suffix
    #[arg(long)]
    web: bool,
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

fn describe_parse_failures(failed_documents: &EaslMultiDocument) -> String {
  let mut descriptions = vec![];
  for (doc, _, source) in &failed_documents.sources {
    for err in &doc.parsing_failures {
      descriptions.push(err.describe(doc, source));
    }
  }
  descriptions.join("\n\n")
}

fn try_compile_easl_file(input: &Path) -> Result<String, String> {
  match compile_easl_file_to_wgsl(input)
    .map_err(|e| format!("IO error reading {}: {}", input.display(), e))?
  {
    Ok(Ok(wgsl)) => Ok(wgsl),
    Ok(Err((documents, errors))) => Err(format!(
      "Compilation failed due to errors:\n\n{}",
      errors.describe(&documents)
    )),
    Err(failed_documents) => Err(format!(
      "Compilation failed due to parsing error:\n\n{}",
      describe_parse_failures(&failed_documents)
    )),
  }
}

#[cfg(feature = "interpreter")]
fn try_get_validated_easl_program(input: &Path) -> Result<Program, String> {
  load_validated_easl_program(input).1
}

/// The files a program was loaded from: its main file and everything that
/// file imports (even when loading stopped at an error), each with the
/// source that was read.
fn document_sources(documents: &EaslMultiDocument) -> Vec<(PathBuf, String)> {
  documents
    .sources
    .iter()
    .map(|(_, path, source)| (PathBuf::from(path), source.clone()))
    .collect()
}

/// The files the program in `input` is made of, as [`document_sources`]
/// (empty if `input` can't be read).
fn program_sources(input: &Path) -> Vec<(PathBuf, String)> {
  match load_easl_program_from_file(input) {
    Ok(Ok((documents, _))) | Ok(Err(documents)) => document_sources(&documents),
    Err(_) => vec![],
  }
}

/// Loads and validates the program in `input`, also returning its
/// [`document_sources`].
#[cfg(feature = "interpreter")]
fn load_validated_easl_program(
  input: &Path,
) -> (Vec<(PathBuf, String)>, Result<Program, String>) {
  match load_easl_program_from_file(input) {
    Err(e) => (
      vec![],
      Err(format!("IO error reading {}: {}", input.display(), e)),
    ),
    Ok(Ok((documents, Ok(mut program)))) => {
      let errors = program.validate_raw_program(CompilerTarget::WGSL);
      let result = if errors.is_empty() {
        Ok(program)
      } else {
        Err(errors.describe(&documents))
      };
      (document_sources(&documents), result)
    }
    Ok(Ok((documents, Err(errors)))) => (
      document_sources(&documents),
      Err(errors.describe(&documents)),
    ),
    Ok(Err(failed_documents)) => (
      document_sources(&failed_documents),
      Err(format!(
        "Compilation failed due to parsing error:\n\n{}",
        describe_parse_failures(&failed_documents)
      )),
    ),
  }
}

/// The files programs were compiled from, watched for changes: a change to
/// any of them (a main file or anything it imports) calls `on_change` with
/// the changed file. A watched directory also reports `.easl` files in it
/// that aren't watched yet (new programs).
struct WatchedSources {
  watcher: RecommendedWatcher,
  /// Each watched file with the contents last seen: what the programs were
  /// compiled from, or a newer version already reported.
  contents: Arc<Mutex<HashMap<PathBuf, String>>>,
  on_change: Arc<dyn Fn(&Path) + Send + Sync>,
}

/// What a file event means for the watched files.
enum FileState {
  Unwatched,
  Unchanged,
  Changed,
}

impl WatchedSources {
  fn new(
    directory: Option<&Path>,
    on_change: impl Fn(&Path) + Send + Sync + 'static,
  ) -> Result<Self, String> {
    let contents = Arc::new(Mutex::new(HashMap::<PathBuf, String>::new()));
    let on_change: Arc<dyn Fn(&Path) + Send + Sync> = Arc::new(on_change);
    let (notify_tx, notify_rx) = channel();
    let mut watcher = RecommendedWatcher::new(notify_tx, Config::default())
      .map_err(|e| format!("Error: Failed to create file watcher\n{}", e))?;
    let directory = match directory {
      Some(directory) => {
        let directory = directory.canonicalize().map_err(|e| {
          format!("Error: Failed to watch path {}\n{}", directory.display(), e)
        })?;
        watcher
          .watch(&directory, RecursiveMode::Recursive)
          .map_err(|e| {
            format!(
              "Error: Failed to watch path {}\n{}",
              directory.display(),
              e
            )
          })?;
        Some(directory)
      }
      None => None,
    };
    {
      let contents = Arc::clone(&contents);
      let on_change = Arc::clone(&on_change);
      std::thread::spawn(move || {
        while let Ok(event) = notify_rx.recv() {
          // Editors save in place (modify events) or by renaming a new
          // file over the old one (create or rename events).
          let Ok(Event {
            kind: EventKind::Modify(_) | EventKind::Create(_),
            paths,
            ..
          }) = event
          else {
            continue;
          };
          for path in paths {
            let Ok(path) = path.canonicalize() else {
              continue;
            };
            match Self::check(&contents, &path) {
              FileState::Changed => on_change(&path),
              FileState::Unwatched
                if directory
                  .as_ref()
                  .is_some_and(|directory| path.starts_with(directory))
                  && path.extension().and_then(|e| e.to_str())
                    == Some("easl") =>
              {
                on_change(&path)
              }
              FileState::Unwatched | FileState::Unchanged => {}
            }
          }
        }
      });
    }
    Ok(Self {
      watcher,
      contents,
      on_change,
    })
  }

  /// Whether watched file `path` differs from its contents last seen,
  /// recording the current contents if so.
  fn check(
    contents: &Mutex<HashMap<PathBuf, String>>,
    path: &Path,
  ) -> FileState {
    let mut contents = contents.lock().unwrap();
    let Some(last) = contents.get_mut(path) else {
      return FileState::Unwatched;
    };
    match fs::read_to_string(path) {
      Ok(current) if current != *last => {
        *last = current;
        FileState::Changed
      }
      _ => FileState::Unchanged,
    }
  }

  /// Watches exactly `sources`, the files the programs were just compiled
  /// from (with the contents they were compiled from); calls `on_change`
  /// for each that has already changed since it was read.
  fn watch(&mut self, sources: Vec<(PathBuf, String)>) {
    let mut contents = self.contents.lock().unwrap();
    for path in contents.keys() {
      if !sources.iter().any(|(source, _)| source == path) {
        let _ = self.watcher.unwatch(path);
      }
    }
    for (path, _) in &sources {
      if !contents.contains_key(path)
        && let Err(e) = self.watcher.watch(path, RecursiveMode::NonRecursive)
      {
        eprintln!("Warning: can't watch {}: {}", path.display(), e);
      }
    }
    *contents = sources.into_iter().collect();
    let paths: Vec<PathBuf> = contents.keys().cloned().collect();
    drop(contents);
    for path in paths {
      if let FileState::Changed = Self::check(&self.contents, &path) {
        (self.on_change)(&path);
      }
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
  web: bool,
) -> Result<(), String> {
  if web {
    return compile_web(&input, output);
  }
  println!("Compiling {}...", input.display());
  match try_compile_easl_file(&input) {
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
  web: bool,
) -> Result<(), String> {
  if web && input.is_dir() {
    return Err(
      "Error: --web compiles a single program; pass its main .easl file"
        .to_string(),
    );
  }
  if watch {
    watch_compile(input, output, web)
  } else {
    compile_once(&input, &output, web)
  }
}

/// `compile --watch`: compiles `input` (a program, or every program in a
/// directory), then recompiles each program whenever one of its files
/// (its main file or anything it imports) changes. In a directory, a new
/// `.easl` file is compiled as a new program.
fn watch_compile(
  input: PathBuf,
  output: Option<PathBuf>,
  web: bool,
) -> Result<(), String> {
  let directory = input.is_dir();
  // Programs are identified by canonical paths, like the files the watcher
  // reports, so a reported file can be matched to the programs it's part of.
  let input = input
    .canonicalize()
    .map_err(|e| format!("Error: Failed to read {}\n{}", input.display(), e))?;
  let program_files = if directory {
    find_easl_files(&input)?
  } else {
    vec![input.clone()]
  };
  let (change_tx, change_rx) = channel::<PathBuf>();
  let mut watched =
    WatchedSources::new(directory.then_some(input.as_path()), move |path| {
      change_tx.send(path.to_path_buf()).ok();
    })?;
  // Each program's main file, with the files it was last compiled from.
  let mut programs: Vec<(PathBuf, Vec<(PathBuf, String)>)> = vec![];
  let compile = |program: &Path| -> Vec<(PathBuf, String)> {
    // The sources are read before compiling: a file that changes in
    // between is newer than what's recorded, so it's compiled again.
    let sources = program_sources(program);
    let output_path = if directory {
      match get_output_path_for_file(program, &input, &output) {
        Ok(path) => Some(path),
        Err(e) => {
          eprintln!("{e}");
          return sources;
        }
      }
    } else {
      output.clone()
    };
    if let Err(e) = compile_single_file(program.to_path_buf(), output_path, web)
    {
      eprintln!("{e}");
    }
    sources
  };
  for file in program_files {
    let file = file.canonicalize().unwrap_or(file);
    let sources = compile(&file);
    programs.push((file, sources));
  }
  let all_sources = |programs: &[(PathBuf, Vec<(PathBuf, String)>)]| {
    programs
      .iter()
      .flat_map(|(_, sources)| sources.iter().cloned())
      .collect()
  };
  watched.watch(all_sources(&programs));
  println!("\nWatching for changes... (Press Ctrl+C to stop)");
  loop {
    let first = change_rx
      .recv()
      .map_err(|e| format!("Error: Channel receive error\n{e}"))?;
    let mut changed = vec![first];
    changed.extend(change_rx.try_iter());
    changed.sort();
    changed.dedup();
    for path in &changed {
      // A new file in the watched directory is a new program.
      if directory
        && !programs.iter().any(|(program, sources)| {
          program == path || sources.iter().any(|(source, _)| source == path)
        })
      {
        programs.push((path.clone(), vec![]));
      }
    }
    for (program, sources) in programs.iter_mut() {
      // A program is stale if it's new or one of its files no longer
      // matches what it was compiled from (a late event can report a
      // change the program was already compiled with).
      let stale = |path: &PathBuf| {
        if sources.is_empty() {
          return path == program;
        }
        sources.iter().any(|(source, compiled)| {
          source == path
            && fs::read_to_string(path)
              .is_ok_and(|current| current != *compiled)
        })
      };
      let Some(path) = changed.iter().find(|path| stale(path)) else {
        continue;
      };
      println!("\n{} changed, recompiling...", path.display());
      let new_sources = compile(program);
      if !new_sources.is_empty() {
        *sources = new_sources;
      }
    }
    watched.watch(all_sources(&programs));
  }
}

fn compile_once(
  input: &PathBuf,
  output: &Option<PathBuf>,
  web: bool,
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

      if let Err(e) = compile_single_file(file.clone(), output_path, web) {
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
    compile_single_file(input.clone(), output_path, web)
  }
}

/// Compiles the program in `input` to a web page in the `output` directory:
/// the page, the program's sources, and the prebuilt web runtime that
/// compiles and runs them in the browser.
#[cfg(feature = "web")]
fn compile_web(input: &Path, output: Option<PathBuf>) -> Result<(), String> {
  const RUNTIME_JS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/easl_web.js"));
  const RUNTIME_WASM: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/easl_web_bg.wasm"));
  println!("Compiling {} for the web...", input.display());
  let bundle = bundle_program(input)
    .map_err(|e| format!("Compilation failed due to errors:\n\n{e}"))?;
  let output_dir = output.unwrap_or_else(|| {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    input.with_file_name(format!("{stem}_web"))
  });
  fs::create_dir_all(&output_dir).map_err(|e| {
    format!(
      "Error: Failed to create directory {}\n{e}",
      output_dir.display()
    )
  })?;
  let files = [
    ("index.html", bundle.index_html.as_bytes()),
    ("easl-program.js", bundle.program_js.as_bytes()),
    ("easl_web.js", RUNTIME_JS),
    ("easl_web_bg.wasm", RUNTIME_WASM),
  ]
  .into_iter()
  .chain(
    RUNTIME_SUPPORT_FILES
      .iter()
      .map(|(name, contents)| (*name, contents.as_bytes())),
  );
  for (name, contents) in files {
    let path = output_dir.join(name);
    fs::write(&path, contents).map_err(|e| {
      format!("Error: Failed to write output file {}\n{e}", path.display())
    })?;
  }
  println!("Finished: {}", output_dir.display());
  Ok(())
}

#[cfg(not(feature = "web"))]
fn compile_web(_input: &Path, _output: Option<PathBuf>) -> Result<(), String> {
  Err(
    "This build of the easl CLI was compiled without web support. Build the \
     CLI with `--features web` to enable `easl compile --web`."
      .to_string(),
  )
}

fn check_single_file(input: PathBuf) -> Result<(), String> {
  print!("Typechecking {}...   ", input.display());
  match try_compile_easl_file(&input) {
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
  let formatted = format_easl_source(&easl_source).map_err(|_| {
    format!(
      "Failed to format {}: source has parse errors",
      input.display()
    )
  })?;
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
  if watch {
    // AtomicBool polled by the IOManager's reload_requested() on every frame.
    let reload_flag = Arc::new(AtomicBool::new(false));

    // Channel used to wake the main loop when the program is not running and
    // we need to block-wait for the next file change.
    let (change_tx, change_rx) = channel::<()>();

    // On any real change to the program's files, the watcher sets the
    // reload flag (signals the running window loop to exit) and sends on
    // change_tx (wakes a blocking wait in the main loop).
    let mut watched = {
      let reload_flag = Arc::clone(&reload_flag);
      WatchedSources::new(None, move |_| {
        reload_flag.store(true, Ordering::Relaxed);
        change_tx.send(()).ok();
      })?
    };

    let mut io = StdoutIO::with_reload_flag(Arc::clone(&reload_flag));

    println!("Watching for changes... (Press Ctrl+C to stop)");

    loop {
      // Reset IO state (clears GPU handle and reload flag) before reading
      // the file, so a change saved while this version compiles still
      // reloads it.
      io.reset_for_reload();

      // Compile current source (re-reads file and chases imports each time),
      // and watch the files it was compiled from.
      let (sources, program) = load_validated_easl_program(&input);
      if !sources.is_empty() {
        watched.watch(sources);
      }
      let program = match program {
        Ok(p) => p,
        Err(e) => {
          eprintln!("Compilation error:\n{e}");
          close_persistent_window();
          change_rx
            .recv()
            .map_err(|e| format!("Watcher disconnected: {e}"))?;
          while change_rx.try_recv().is_ok() {}
          continue;
        }
      };

      // The audio stream deliberately persists across reloads — the new
      // run's `start-audio` hot-swaps the running stream's program instead
      // of tearing it down (no device re-open, no gap in playback). But if
      // the reloaded program has no audio entry at all, nothing will ever
      // swap it, so stop the stale stream here.
      if program
        .find_fn_names_by_entry_point(|e| e == EntryPoint::Audio)
        .is_empty()
      {
        easl::audio::stop_audio();
      }

      // Run the program.  The same `io` is reused across reloads so the
      // reload_flag Arc is still wired up.
      match run_program_entry_with_io_from_path(
        program,
        entry.as_deref(),
        io,
        &input,
      ) {
        Err(e) => {
          eprintln!("Runtime error: {e:?}");
          close_persistent_window();
          // Rebuild io since it was consumed.
          io = StdoutIO::with_reload_flag(Arc::clone(&reload_flag));
          change_rx
            .recv()
            .map_err(|e| format!("Watcher disconnected: {e}"))?;
          while change_rx.try_recv().is_ok() {}
        }
        Ok((returned_io, did_reload)) => {
          io = returned_io;
          if did_reload {
            // File already changed — drain any buffered notifications.
            while change_rx.try_recv().is_ok() {}
            println!("\n{} changed, reloading...", input.display());
          } else {
            // Program finished on its own (e.g. user closed the window).
            // Exit the process rather than keeping the watcher alive.
            close_persistent_window();
            return Ok(());
          }
        }
      }
    }
  } else {
    let program = try_get_validated_easl_program(&input)?;
    match run_program_entry_from_path(
      program,
      entry.as_ref().map(|s| s.as_str()),
      &input,
    ) {
      Err(e) => return Err(format!("{e:?}")),
      _ => {}
    }
    Ok(())
  }
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
      web,
    } => compile_file(input, output, watch, web),
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
