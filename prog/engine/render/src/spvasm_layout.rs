//! Read the member offsets a shader actually declares.
//!
//! The layout tests used to assert the Rust offsets against numbers typed into
//! the same file, which proves only that the struct did not move — never that it
//! still agrees with the shader. When they disagree the GPU reads the wrong
//! bytes, validation stays silent (the block is the right *size*), and what you
//! get is a plausible-looking wrong image. That is the most expensive kind of bug
//! in this tree, so the tests now parse the `.spvasm` and compare.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// `OpMemberDecorate %<block> <member> Offset <bytes>`, in member order.
///
/// `path` is relative to the workspace root.
pub fn member_offsets(path: &str, block: &str) -> BTreeMap<u32, u32> {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // prog/engine/render -> workspace root
    for _ in 0..3 {
        root.pop();
    }
    let full = root.join(path);
    let text = std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()));

    let want = format!("%{block}");
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() != Some("OpMemberDecorate") || it.next() != Some(want.as_str()) {
            continue;
        }
        let Some(member) = it.next().and_then(|m| m.parse::<u32>().ok()) else {
            continue;
        };
        if it.next() != Some("Offset") {
            continue;
        }
        if let Some(off) = it.next().and_then(|o| o.parse::<u32>().ok()) {
            out.insert(member, off);
        }
    }
    assert!(
        !out.is_empty(),
        "no `OpMemberDecorate %{block} .. Offset ..` in {}",
        full.display()
    );
    out
}

/// Panic unless every `layout(offset = N)` in a GLSL source is a real offset.
///
/// A GLSL block declares only the members the shader reads, so member *indices*
/// carry no meaning across the two forms — the byte offsets do. Checking those
/// keeps a GLSL shader inside the same safety net as the assembly it replaced.
pub fn assert_glsl_offsets(path: &str, expected: &[(u32, u32)]) {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for _ in 0..3 {
        root.pop();
    }
    let full = root.join(path);
    let text = std::fs::read_to_string(&full)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()));

    let valid: Vec<u32> = expected.iter().map(|(_, off)| *off).collect();
    let mut found = 0;
    for (line_no, line) in text.lines().enumerate() {
        let Some(rest) = line.split("layout(offset").nth(1) else {
            continue;
        };
        let Some(num) = rest.trim_start_matches([' ', '=']).split(')').next() else {
            continue;
        };
        let Ok(off) = num.trim().parse::<u32>() else {
            continue;
        };
        assert!(
            valid.contains(&off),
            "{path}:{}: layout(offset = {off}) is not a member offset of the Rust struct. \
             Valid: {valid:?}",
            line_no + 1
        );
        found += 1;
    }
    assert!(found > 0, "{path}: no `layout(offset = ...)` found — is it still a UBO block?");
}

/// Panic unless the shader declares exactly `expected`, member for member.
///
/// `expected` is `(member index, byte offset)` for every member the *Rust* side
/// defines. A shader may legitimately stop early — a pass that never reads the
/// tail declares only the prefix it uses — so trailing members it omits are fine.
/// A member it declares at a different offset is not.
pub fn assert_prefix_matches(path: &str, block: &str, expected: &[(u32, u32)]) {
    let declared = member_offsets(path, block);
    let want: BTreeMap<u32, u32> = expected.iter().copied().collect();
    for (member, offset) in &declared {
        match want.get(member) {
            Some(rust) => assert_eq!(
                offset,
                rust,
                "{path}: %{block} member {member} is at {offset} in the shader \
                 but {rust} in the Rust struct"
            ),
            None => panic!(
                "{path}: %{block} declares member {member} (offset {offset}) \
                 that the Rust struct does not have"
            ),
        }
    }
}
