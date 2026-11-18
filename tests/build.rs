//! The Makefile: where `make install` puts the program, and what `make static` checks.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A directory of a test, removed with it.
struct Fixture(PathBuf);

impl Fixture {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A directory with the Makefile and a script that stands for Cargo: what it builds as the
/// release is a copy of `release`.
fn project(release: &Path) -> Fixture {
    // The tests run at the same time: each has a directory of its own.
    static PROJECTS: AtomicUsize = AtomicUsize::new(0);
    let directory = Fixture(std::env::temp_dir().join(format!(
        "complex-drawer-make-{}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        PROJECTS.fetch_add(1, Ordering::SeqCst)
    )));
    std::fs::create_dir(directory.path()).unwrap();
    let root = directory.path();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/Makefile"),
        root.join("Makefile"),
    )
    .unwrap();
    let cargo = root.join("cargo");
    std::fs::write(
        &cargo,
        format!(
            "#!/bin/sh\necho \"$*\" >> cargo.calls\ncase \"$1\" in\n\
             build) mkdir -p target/release && cp '{}' target/release/complex_surface_viewer ;;\n\
             clean) rm -rf target ;;\nesac\n",
            release.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    directory
}

fn make(root: &Path, arguments: &[&str]) -> Output {
    Command::new("make")
        .current_dir(root)
        .arg("CARGO=./cargo")
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn make_install_puts_the_checked_release_in_three_places_and_clean_leaves_them() {
    // This test is a program like the one of the release: it needs only the C library.
    let release = std::env::current_exe().unwrap();
    let directory = project(&release);
    let root = directory.path();
    let installed = root.join("a place/with spaces");
    let output = make(
        root,
        &["install", &format!("INSTALL_DIR={}", installed.display())],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Only the C library of the system is dynamic"));
    let copies = [
        root.join("complex_surface_viewer"),
        root.join("complex_surface_viewer.static"),
        installed.join("complex_surface_viewer"),
    ];
    let expected = std::fs::read(&release).unwrap();
    for copy in &copies {
        assert_eq!(std::fs::read(copy).unwrap(), expected, "{copy:?}");
        let mode = std::fs::metadata(copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "{copy:?}");
    }
    assert_eq!(
        std::fs::read_to_string(root.join("cargo.calls")).unwrap(),
        "build --locked --release -j 4\n"
    );

    assert!(make(root, &["clean"]).status.success());
    assert!(!root.join("target").exists());
    assert!(copies.iter().all(|copy| copy.exists()));
}

#[test]
fn make_static_refuses_a_release_whose_libraries_it_cannot_list() {
    let directory = project(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Makefile")));
    let root = directory.path();
    let installed = root.join("installed");
    for target in ["static", "install"] {
        let output = make(
            root,
            &[target, &format!("INSTALL_DIR={}", installed.display())],
        );
        assert!(!output.status.success(), "{target}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("target/release/complex_surface_viewer"),
            "{target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(!root.join("complex_surface_viewer").exists() && !installed.exists());
}
