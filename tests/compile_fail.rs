use std::{fs, path::PathBuf, process::Command};

#[test]
fn invalid_interfaces_have_useful_diagnostics() {
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let library = fs::read_dir(&deps)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("libnative_vtable-")
                && p.extension().is_some_and(|e| e == "rlib")
        })
        .max_by_key(|p| p.metadata().unwrap().modified().unwrap())
        .expect("runtime rlib");
    let temp = std::env::temp_dir().join(format!("native-vtable-ui-{}", std::process::id()));
    fs::create_dir_all(&temp).unwrap();
    let cases = [
        ("duplicate", "duplicate or shadowed virtual method"),
        ("generic", "non-generic"),
        ("by_value", "expected &self or &mut self receiver"),
        ("multiple_parents", "only one native parent"),
        ("overlap", "slot overlaps"),
        ("unknown_override", "method does not belong"),
        ("signature", "incompatible type for trait"),
        ("sibling", "PrefixOf"),
        ("associated", "only virtual methods"),
    ];
    for (case, expected) in cases {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui")
            .join(format!("{case}.rs"));
        let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
            .arg(source)
            .args(["--edition=2024", "--crate-type=lib", "--emit=metadata"])
            .arg("--out-dir")
            .arg(&temp)
            .arg("-L")
            .arg(format!("dependency={}", deps.display()))
            .arg("--extern")
            .arg(format!("native_vtable={}", library.display()))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{case} unexpectedly compiled");
        assert!(
            stderr.contains(expected),
            "{case}: expected {expected:?}\n{stderr}"
        );
    }
    fs::remove_dir_all(temp).unwrap();
}
