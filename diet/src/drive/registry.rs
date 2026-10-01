//! The substrate registry, read where a session needs a substrate's identity
//! (#157, Q2 as ruled).
//!
//! `substrates/registry.toml` is the data seat's file. It names each
//! substrate by id, with the engine, the weights and the equipment behind it.
//! A record's `start.regime` must name a substrate by a TYPED identity -- a
//! weights digest, a hosted model, or a canned server's acts -- and the
//! registry is where that identity is declared. So a session that serves a
//! live endpoint resolves its regimen's substrate id here, and refuses to
//! start when the registry cannot say what the record has to.
//!
//! # A scan, not a TOML reader
//!
//! diet has no TOML reader (the regimen is its own format), and this reads
//! the registry the way #162's registry tests do. By rule, every field read
//! here stays a one-line `key = "value"` string -- `equipment`,
//! `engine_name`, `engine_identity`, `engine_commit`, `weights_kind`,
//! `weights_acts_sha256`, `weights_main` and `hardware_fingerprint` -- and a
//! field written another way is not read. A one-line LIST is recorded as a
//! list, never mistaken for an absent key, because a list is exactly what a
//! multi-shard `weights_main` is. Lines inside a `"""` string are skipped.

use std::collections::{BTreeMap, BTreeSet};

use crate::formats::record::{Engine, Weights};

/// The registry this program was built with: one copy, read at build time,
/// so a binary and the registry it resolves against cannot be two files.
pub const REGISTRY: &str = include_str!("../../../substrates/registry.toml");

/// The sha256 of [`REGISTRY`]'s text: which registry answered, for a reader
/// who has the record and not the binary (ruled on #204).
#[must_use]
pub fn registry_sha256() -> String {
    crate::digest::sha256_hex(REGISTRY.as_bytes())
}

/// One `[kind.id]` table, as the scan reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// Its one-line string values.
    pub strings: BTreeMap<String, String>,
    /// The keys bound to a one-line list.
    pub lists: BTreeSet<String>,
}

/// Every `[kind.id]` table of a TOML document. See the module's doc for what
/// the scan reads, and what it does not.
#[must_use]
pub fn tables(document: &str) -> BTreeMap<String, Table> {
    let mut tables: BTreeMap<String, Table> = BTreeMap::new();
    let mut current = String::new();
    let mut in_long_string = false;
    for line in document.lines() {
        if line.matches("\"\"\"").count() % 2 == 1 {
            in_long_string = !in_long_string;
            continue;
        }
        if in_long_string {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // A header, read strictly: a trailing comment and surrounding
            // space are allowed, and quotes around an id are dropped
            // (`[substrate."bge-small-en-v1.5"]`). One it cannot read gets a
            // table of its own, so the keys under it are never filed under
            // the table before it.
            let header = trimmed
                .split_once('#')
                .map_or(trimmed, |(header, _)| header)
                .trim();
            current = header
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
                .map_or_else(
                    || format!("<unreadable header: {trimmed}>"),
                    |name| name.replace('"', ""),
                );
            continue;
        }
        if let Some((key, _)) = line.split_once(" = [") {
            tables
                .entry(current.clone())
                .or_default()
                .lists
                .insert(key.trim().to_owned());
            continue;
        }
        let Some((key, value)) = line.split_once(" = \"") else {
            continue;
        };
        let Some(value) = value.strip_suffix('"') else {
            continue;
        };
        tables
            .entry(current.clone())
            .or_default()
            .strings
            .insert(key.trim().to_owned(), value.to_owned());
    }
    tables
}

/// What the registry says a substrate is, in the record's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The engine: its name, and the digest the registry pins it by.
    pub engine: Engine,
    /// The weights, typed.
    pub weights: Weights,
    /// The equipment's `hardware_fingerprint`.
    pub hardware_fingerprint: String,
    /// The serving checkout's commit, when the registry records one: what an
    /// endpoint's own report of its build is compared against.
    pub engine_commit: Option<String>,
    /// The chat template's digest, when the registry declares one.
    pub chat_template_sha256: Option<String>,
}

/// The identity the registry in `document` gives substrate `id`.
///
/// # Errors
///
/// When `id` is not a registered substrate; when its entry lacks a field the
/// record requires; or when its main weights are several files, which the
/// record's `weights` cannot spell (#92).
pub fn identity(document: &str, id: &str) -> Result<Identity, String> {
    let tables = tables(document);
    let table_name = format!("substrate.{id}");
    let Some(table) = tables.get(&table_name) else {
        return Err(format!(
            "`{id}` is not a substrate in the registry: a record names a substrate by an \
             identity the registry declares, and this one declares none"
        ));
    };
    let field = |key: &str| {
        table
            .strings
            .get(key)
            .cloned()
            .ok_or_else(|| format!("the registry's `[{table_name}]` has no one-line `{key}`"))
    };
    let canned = table.strings.get("weights_kind").map(String::as_str) == Some("canned");
    let weights = if canned {
        Weights::Canned {
            acts_sha256: field("weights_acts_sha256")?,
        }
    } else if table.lists.contains("weights_main") {
        return Err(format!(
            "`{id}`'s `weights_main` is a list of shards, and the record's `weights` holds \
             one digest. No digest of digests is minted here: the server never reports \
             one. It waits on the record change requested on #92 -- `Weights::Digest` \
             widened to a list, or a shards kind"
        ));
    } else {
        Weights::Digest(field("weights_main")?)
    };
    let equipment = field("equipment")?;
    let hardware_fingerprint = tables
        .get(&format!("equipment.{equipment}"))
        .and_then(|equipment| equipment.strings.get("hardware_fingerprint"))
        .cloned()
        .ok_or_else(|| {
            format!("`{id}`'s equipment `{equipment}` has no one-line `hardware_fingerprint`")
        })?;
    Ok(Identity {
        engine: Engine {
            name: field("engine_name")?,
            version_or_digest: field("engine_identity")?,
        },
        weights,
        hardware_fingerprint,
        engine_commit: table.strings.get("engine_commit").cloned(),
        chat_template_sha256: table.strings.get("chat_template_sha256").cloned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registered_digest_substrate_resolves_to_what_the_registry_declares() {
        let found =
            identity(REGISTRY, "accel24-beellama-qwen27b-q4kxl").expect("a registered substrate");
        let table = &tables(REGISTRY)["substrate.accel24-beellama-qwen27b-q4kxl"];
        assert_eq!(
            found.weights,
            Weights::Digest(table.strings["weights_main"].clone())
        );
        assert_eq!(found.engine.name, table.strings["engine_name"]);
        assert_eq!(
            found.engine.version_or_digest,
            table.strings["engine_identity"]
        );
        assert_eq!(
            found.hardware_fingerprint,
            tables(REGISTRY)[&format!("equipment.{}", table.strings["equipment"])].strings["hardware_fingerprint"]
        );
    }

    #[test]
    fn the_canned_instance_resolves_to_the_acts_this_crate_plays() {
        let found = identity(REGISTRY, "canned-cache-n").expect("registered");
        assert_eq!(
            found.weights,
            Weights::Canned {
                acts_sha256: crate::drive::canned::acts_digest()
            }
        );
    }

    #[test]
    fn an_unregistered_substrate_is_refused_by_name() {
        let refused = identity(REGISTRY, "nowhere-at-all").expect_err("not registered");
        assert!(
            refused.contains("`nowhere-at-all` is not a substrate"),
            "{refused}"
        );
    }

    #[test]
    fn a_main_of_several_shards_is_refused_and_names_the_record_change() {
        // The DoD 1 instance: two shards, which `Weights::Digest` cannot hold.
        let refused =
            identity(REGISTRY, "ada48-llamacpp-qwen38flashnext-q20").expect_err("two shards");
        assert!(
            refused.contains("list of shards") && refused.contains("#92"),
            "{refused}"
        );
    }

    #[test]
    fn a_registered_substrates_commit_and_chat_template_are_read() {
        let id = "accel24-llamacpp-qwen38-27b-iq3s";
        let table = &tables(REGISTRY)[&format!("substrate.{id}")];
        let found = identity(REGISTRY, id).expect("registered, one main file");
        assert_eq!(
            found.engine_commit.as_deref(),
            Some(table.strings["engine_commit"].as_str())
        );
        assert_eq!(
            found.chat_template_sha256.as_deref(),
            Some(table.strings["chat_template_sha256"].as_str())
        );
    }

    #[test]
    fn a_header_is_read_strictly_and_never_files_keys_under_the_table_before_it() {
        let scanned = tables(
            "[substrate.a]\nweights_main = \"AAA\"\n\
             [substrate.b] # a comment\nweights_main = \"BBB\"\n\
               [substrate.\"c.d\"]\nweights_main = \"CCC\"\n\
             [substrate.e\nweights_main = \"EEE\"\n",
        );
        assert_eq!(scanned["substrate.a"].strings["weights_main"], "AAA");
        assert_eq!(scanned["substrate.b"].strings["weights_main"], "BBB");
        assert_eq!(scanned["substrate.c.d"].strings["weights_main"], "CCC");
        assert!(
            scanned
                .iter()
                .any(|(name, table)| name.starts_with("<unreadable header")
                    && table.strings["weights_main"] == "EEE"),
            "an unreadable header's keys are kept apart: {scanned:?}"
        );
        // The registry's own quoted ids resolve by their names.
        assert!(tables(REGISTRY).contains_key("substrate.bge-small-en-v1.5"));
    }

    #[test]
    fn the_registry_digest_is_the_included_texts() {
        assert_eq!(
            registry_sha256(),
            crate::digest::sha256_hex(
                std::fs::read(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../substrates/registry.toml"
                ))
                .expect("the registry")
                .as_slice()
            )
        );
    }

    #[test]
    fn the_scan_reads_a_list_as_a_list_and_never_as_an_absent_key() {
        let scanned = tables("[substrate.x]\nweights_main = [\"a\", \"b\"]\nengine_name = \"e\"\n");
        let table = &scanned["substrate.x"];
        assert!(table.lists.contains("weights_main"));
        assert!(!table.strings.contains_key("weights_main"));
        assert_eq!(table.strings["engine_name"], "e");
    }
}
