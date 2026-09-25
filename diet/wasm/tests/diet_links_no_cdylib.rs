//! The reason this crate exists, held as a test (#32): `discipline-diet`
//! links no `cdylib`. While the wasm surface lived inside that crate, its
//! `crate-type` carried `cdylib` unconditionally -- Cargo cannot make it
//! conditional -- and every native build linked a 40 MB `libdiet.so` nothing
//! loaded. Moving the surface here only stays a fix while nobody puts the
//! `cdylib` back, so this is not feature-gated: it runs in the workspace-wide
//! test `verify.sh` performs.
//!
//! Asked of `cargo metadata` rather than read out of `diet/Cargo.toml` by
//! hand, so the verdict is Cargo's own reading of the manifest, not a second
//! reader of TOML that could disagree with it.

use std::path::Path;
use std::process::Command;

#[test]
fn discipline_diet_links_no_cdylib() {
    let workspace_manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&workspace_manifest)
        .output()
        .unwrap_or_else(|err| panic!("could not run cargo metadata: {err}"));
    assert!(
        output.status.success(),
        "cargo metadata failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");

    let lib_crate_types: Vec<&str> = metadata["packages"]
        .as_array()
        .expect("cargo metadata lists packages")
        .iter()
        .filter(|package| package["name"] == "discipline-diet")
        .flat_map(|package| {
            package["targets"]
                .as_array()
                .expect("a package lists targets")
        })
        .filter(|target| {
            target["kind"].as_array().is_some_and(|kinds| {
                kinds.iter().any(|kind| {
                    ["lib", "rlib", "dylib", "cdylib", "staticlib"]
                        .contains(&kind.as_str().expect("a target kind is a string"))
                })
            })
        })
        .flat_map(|target| {
            target["crate_types"]
                .as_array()
                .expect("a target lists crate types")
        })
        .map(|crate_type| crate_type.as_str().expect("a crate type is a string"))
        .collect();

    assert!(
        !lib_crate_types.is_empty(),
        "found no library target for discipline-diet; this test would assert nothing"
    );
    assert!(
        !lib_crate_types.contains(&"cdylib"),
        "discipline-diet's library links a cdylib again ({lib_crate_types:?}); the wasm \
         surface's cdylib belongs to discipline-diet-wasm alone (#32)"
    );
}
