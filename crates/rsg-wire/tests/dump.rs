//! WIRE-02 step 1 (D-15): write every Rust-encoded fixture case to `$DUMP_DIR` so
//! `python/tests/test_wire_decode.py` can decode it with upstream's real decoder.
//! Run both steps with `scripts/check_wire_decode.sh`.

#[allow(dead_code)] // shared test helpers; this binary uses only some of them
mod common;

#[test]
fn dump_rust_encodings() {
    let Some(dir) = std::env::var_os("DUMP_DIR") else {
        println!("DUMP_DIR unset; nothing dumped");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));

    // Encoded from hand-built Rust values, not copied from the fixtures: these are the bytes
    // Rust would put on the wire.
    let mut written = 0;
    for (name, value) in common::cases() {
        let path = dir.join(format!("{name}.msgpack"));
        std::fs::write(&path, value.encode())
            .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        written += 1;
    }

    let expected = common::manifest()["cases"]
        .as_array()
        .expect("manifest cases array")
        .len();
    assert_eq!(
        written, expected,
        "dumped case count != manifest case count"
    );
    println!("dumped {written} cases to {}", dir.display());
}
