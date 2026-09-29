//! With the `web` feature, builds easl's web runtime (the `web/` crate in
//! the easl repository) to WebAssembly and generates its JS bindings, so
//! `easl compile --web` can embed them. Programs compile in the browser, so
//! this runtime is built once, with the CLI.

fn main() {
  #[cfg(feature = "web")]
  web::build_runtime();
}

#[cfg(feature = "web")]
mod web {
  use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
  };

  use wasm_bindgen_cli_support::Bindgen;

  /// Variables cargo sets for build scripts, besides the `CARGO_*` ones.
  const BUILD_SCRIPT_VARS: &[&str] = &[
    "TARGET",
    "HOST",
    "OUT_DIR",
    "OPT_LEVEL",
    "PROFILE",
    "DEBUG",
    "NUM_JOBS",
    "RUSTC",
    "RUSTDOC",
    "RUSTC_LINKER",
    "RUSTFLAGS",
  ];

  pub fn build_runtime() {
    let easl_dir = easl_crate_dir();
    let web_dir = easl_dir.join("web");
    for path in [
      easl_dir.join("src"),
      easl_dir.join("Cargo.toml"),
      web_dir.join("src"),
      web_dir.join("templates"),
      web_dir.join("Cargo.toml"),
    ] {
      println!("cargo:rerun-if-changed={}", path.display());
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out_dir.join("web-target");
    // The runtime is its own build, configured by `web/.cargo/config.toml`
    // (target and linker flags) — none of this build's settings apply.
    // Cargo describes this build script's target in environment variables
    // (`CARGO_CFG_UNIX`, `TARGET`, ...), which the nested build would
    // otherwise pass on to its own build scripts.
    let mut cargo = Command::new(env::var("CARGO").unwrap());
    cargo
      .current_dir(&web_dir)
      .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
      .arg("--target-dir")
      .arg(&target_dir);
    for (name, _) in env::vars_os() {
      let name = name.to_string_lossy();
      if (name.starts_with("CARGO_") && name != "CARGO_HOME")
        || BUILD_SCRIPT_VARS.contains(&&*name)
      {
        cargo.env_remove(&*name);
      }
    }
    let status = cargo
      .status()
      .expect("failed to run cargo to build the easl web runtime");
    if !status.success() {
      panic!(
        "building the easl web runtime failed. It needs the wasm32 target \
         (`rustup target add wasm32-unknown-unknown`); to build the CLI \
         without `easl compile --web`, disable the `web` feature \
         (`--no-default-features --features interpreter`)."
      );
    }

    Bindgen::new()
      .input_path(
        target_dir.join("wasm32-unknown-unknown/release/easl_web.wasm"),
      )
      .web(true)
      .unwrap()
      .typescript(false)
      .omit_default_module_path(false)
      .generate(&out_dir)
      .expect("failed to generate the easl web runtime's JS bindings");
  }

  /// The easl crate's source directory, wherever cargo resolved it (a path
  /// or git dependency).
  fn easl_crate_dir() -> PathBuf {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let output = Command::new(env::var("CARGO").unwrap())
      .args(["metadata", "--format-version", "1", "--manifest-path"])
      .arg(Path::new(&manifest_dir).join("Cargo.toml"))
      .output()
      .expect("failed to run cargo metadata");
    assert!(
      output.status.success(),
      "cargo metadata failed:\n{}",
      String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
      serde_json::from_slice(&output.stdout).unwrap();
    let easl_manifest = metadata["packages"]
      .as_array()
      .unwrap()
      .iter()
      .find(|package| package["name"] == "easl")
      .expect("cargo metadata doesn't list the easl crate")["manifest_path"]
      .as_str()
      .unwrap();
    Path::new(easl_manifest).parent().unwrap().to_path_buf()
  }
}
