use std;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use git2;
use support::registry::Package;
use support::{basic_manifest, git, is_nightly, path2url, paths, project, registry};
use support::{cargo_process, sleep_ms};
use tar::Archive;

#[test]
fn simple() {
    let p = project()
        .file("Cargo.toml", r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            exclude = ["*.txt"]
            license = "MIT"
            description = "foo"
        "#)
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .file("src/bar.txt", "") // should be ignored when packaging
        .build();

    p.cargo("package")
        .with_stderr(
            "\
[WARNING] manifest has no documentation[..]
See [..]
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();
    assert!(p.root().join("target/package/foo-0.0.1.crate").is_file());
    p.cargo("package -l")
        .with_stdout(
            "\
Cargo.toml
src/main.rs
",
        ).run();
    p.cargo("package").with_stdout("").run();

    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    for f in ar.entries().unwrap() {
        let f = f.unwrap();
        let fname = f.header().path_bytes();
        let fname = &*fname;
        assert!(
            fname == b"foo-0.0.1/Cargo.toml"
                || fname == b"foo-0.0.1/Cargo.toml.orig"
                || fname == b"foo-0.0.1/src/main.rs",
            "unexpected filename: {:?}",
            f.header().path()
        )
    }
}

#[test]
fn metadata_warning() {
    let p = project().file("src/main.rs", "fn main() {}").build();
    p.cargo("package")
        .with_stderr(
            "\
warning: manifest has no description, license, license-file, documentation, \
homepage or repository.
See http://doc.crates.io/manifest.html#package-metadata for more info.
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
        "#,
        ).file("src/main.rs", "fn main() {}")
        .build();
    p.cargo("package")
        .with_stderr(
            "\
warning: manifest has no description, documentation, homepage or repository.
See http://doc.crates.io/manifest.html#package-metadata for more info.
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            repository = "bar"
        "#,
        ).file("src/main.rs", "fn main() {}")
        .build();
    p.cargo("package")
        .with_stderr(
            "\
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();
}

#[test]
fn package_verbose() {
    let root = paths::root().join("all");
    let repo = git::repo(&root)
        .file("Cargo.toml", &basic_manifest("foo", "0.0.1"))
        .file("src/main.rs", "fn main() {}")
        .file("a/Cargo.toml", &basic_manifest("a", "0.0.1"))
        .file("a/src/lib.rs", "")
        .build();
    cargo_process("build").cwd(repo.root()).run();

    println!("package main repo");
    cargo_process("package -v --no-verify")
        .cwd(repo.root())
        .with_stderr(
            "\
[WARNING] manifest has no description[..]
See http://doc.crates.io/manifest.html#package-metadata for more info.
[PACKAGING] foo v0.0.1 ([..])
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] .cargo_vcs_info.json
",
        ).run();

    let f = File::open(&repo.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    let mut entry = ar
        .entries()
        .unwrap()
        .map(|f| f.unwrap())
        .find(|e| e.path().unwrap().ends_with(".cargo_vcs_info.json"))
        .unwrap();
    let mut contents = String::new();
    entry.read_to_string(&mut contents).unwrap();
    assert_eq!(
        &contents[..],
        &*format!(
            r#"{{
  "git": {{
    "sha1": "{}"
  }}
}}
"#,
            repo.revparse_head()
        )
    );

    println!("package sub-repo");
    cargo_process("package -v --no-verify")
        .cwd(repo.root().join("a"))
        .with_stderr(
            "\
[WARNING] manifest has no description[..]
See http://doc.crates.io/manifest.html#package-metadata for more info.
[PACKAGING] a v0.0.1 ([..])
[ARCHIVING] Cargo.toml
[ARCHIVING] src/lib.rs
[ARCHIVING] .cargo_vcs_info.json
",
        ).run();
}

#[test]
fn package_verification() {
    let p = project().file("src/main.rs", "fn main() {}").build();
    p.cargo("build").run();
    p.cargo("package")
        .with_stderr(
            "\
[WARNING] manifest has no description[..]
See http://doc.crates.io/manifest.html#package-metadata for more info.
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();
}

#[test]
fn vcs_file_collision() {
    let p = project().build();
    let _ = git::repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            description = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
            exclude = ["*.no-existe"]
        "#,
        ).file(
            "src/main.rs",
            r#"
            fn main() {}
        "#,
        ).file(".cargo_vcs_info.json", "foo")
        .build();
    p.cargo("package")
        .arg("--no-verify")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] Invalid inclusion of reserved file name .cargo_vcs_info.json \
in package source
",
        ).run();
}

#[test]
fn path_dependency_no_version() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"

            [dependencies.bar]
            path = "bar"
        "#,
        ).file("src/main.rs", "fn main() {}")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.1.0"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("package")
        .with_status(101)
        .with_stderr(
            "\
[WARNING] manifest has no documentation, homepage or repository.
See http://doc.crates.io/manifest.html#package-metadata for more info.
[ERROR] all path dependencies must have a version specified when packaging.
dependency `bar` does not specify a version.
",
        ).run();
}

#[test]
fn exclude() {
    let p = project()
        .file("Cargo.toml", r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            exclude = [
                "*.txt",
                # file in root
                "file_root_1",       # NO_CHANGE (ignored)
                "/file_root_2",      # CHANGING (packaged -> ignored)
                "file_root_3/",      # NO_CHANGE (packaged)
                "file_root_4/*",     # NO_CHANGE (packaged)
                "file_root_5/**",    # NO_CHANGE (packaged)
                # file in sub-dir
                "file_deep_1",       # CHANGING (packaged -> ignored)
                "/file_deep_2",      # NO_CHANGE (packaged)
                "file_deep_3/",      # NO_CHANGE (packaged)
                "file_deep_4/*",     # NO_CHANGE (packaged)
                "file_deep_5/**",    # NO_CHANGE (packaged)
                # dir in root
                "dir_root_1",        # CHANGING (packaged -> ignored)
                "/dir_root_2",       # CHANGING (packaged -> ignored)
                "dir_root_3/",       # CHANGING (packaged -> ignored)
                "dir_root_4/*",      # NO_CHANGE (ignored)
                "dir_root_5/**",     # NO_CHANGE (ignored)
                # dir in sub-dir
                "dir_deep_1",        # CHANGING (packaged -> ignored)
                "/dir_deep_2",       # NO_CHANGE
                "dir_deep_3/",       # CHANGING (packaged -> ignored)
                "dir_deep_4/*",      # CHANGING (packaged -> ignored)
                "dir_deep_5/**",     # CHANGING (packaged -> ignored)
            ]
        "#)
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .file("bar.txt", "")
        .file("src/bar.txt", "")
        // file in root
        .file("file_root_1", "")
        .file("file_root_2", "")
        .file("file_root_3", "")
        .file("file_root_4", "")
        .file("file_root_5", "")
        // file in sub-dir
        .file("some_dir/file_deep_1", "")
        .file("some_dir/file_deep_2", "")
        .file("some_dir/file_deep_3", "")
        .file("some_dir/file_deep_4", "")
        .file("some_dir/file_deep_5", "")
        // dir in root
        .file("dir_root_1/some_dir/file", "")
        .file("dir_root_2/some_dir/file", "")
        .file("dir_root_3/some_dir/file", "")
        .file("dir_root_4/some_dir/file", "")
        .file("dir_root_5/some_dir/file", "")
        // dir in sub-dir
        .file("some_dir/dir_deep_1/some_dir/file", "")
        .file("some_dir/dir_deep_2/some_dir/file", "")
        .file("some_dir/dir_deep_3/some_dir/file", "")
        .file("some_dir/dir_deep_4/some_dir/file", "")
        .file("some_dir/dir_deep_5/some_dir/file", "")
        .build();

    p.cargo("package --no-verify -v")
        .with_stdout("")
        .with_stderr(
            "\
[WARNING] manifest has no description[..]
See http://doc.crates.io/manifest.html#package-metadata for more info.
[WARNING] [..] file `dir_root_1/some_dir/file` WILL be excluded [..]
See [..]
[WARNING] [..] file `dir_root_2/some_dir/file` WILL be excluded [..]
See [..]
[WARNING] [..] file `dir_root_3/some_dir/file` WILL be excluded [..]
See [..]
[WARNING] [..] file `some_dir/dir_deep_1/some_dir/file` WILL be excluded [..]
See [..]
[WARNING] [..] file `some_dir/dir_deep_3/some_dir/file` WILL be excluded [..]
See [..]
[WARNING] [..] file `some_dir/file_deep_1` WILL be excluded [..]
See [..]
[WARNING] No (git) Cargo.toml found at `[..]` in workdir `[..]`
[PACKAGING] foo v0.0.1 ([..])
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
",
        ).run();

    assert!(p.root().join("target/package/foo-0.0.1.crate").is_file());

    p.cargo("package -l")
        .with_stdout(
            "\
Cargo.toml
dir_root_1/some_dir/file
dir_root_2/some_dir/file
dir_root_3/some_dir/file
file_root_3
file_root_4
file_root_5
some_dir/dir_deep_1/some_dir/file
some_dir/dir_deep_2/some_dir/file
some_dir/dir_deep_3/some_dir/file
some_dir/dir_deep_4/some_dir/file
some_dir/dir_deep_5/some_dir/file
some_dir/file_deep_1
some_dir/file_deep_2
some_dir/file_deep_3
some_dir/file_deep_4
some_dir/file_deep_5
src/main.rs
",
        ).run();
}

#[test]
fn include() {
    let p = project()
        .file("Cargo.toml", r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            exclude = ["*.txt"]
            include = ["foo.txt", "**/*.rs", "Cargo.toml"]
        "#)
        .file("foo.txt", "")
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .file("src/bar.txt", "") // should be ignored when packaging
        .build();

    p.cargo("package --no-verify -v")
        .with_stderr(
            "\
[WARNING] manifest has no description[..]
See http://doc.crates.io/manifest.html#package-metadata for more info.
[WARNING] No (git) Cargo.toml found at `[..]` in workdir `[..]`
[PACKAGING] foo v0.0.1 ([..])
[ARCHIVING] [..]
[ARCHIVING] [..]
[ARCHIVING] [..]
",
        ).run();
}

#[test]
fn package_lib_with_bin() {
    let p = project()
        .file("src/main.rs", "extern crate foo; fn main() {}")
        .file("src/lib.rs", "")
        .build();

    p.cargo("package -v").run();
}

#[test]
fn package_git_submodule() {
    let project = git::new("foo", |project| {
        project
            .file(
                "Cargo.toml",
                r#"
                    [project]
                    name = "foo"
                    version = "0.0.1"
                    authors = ["foo@example.com"]
                    license = "MIT"
                    description = "foo"
                    repository = "foo"
                "#,
            ).file("src/lib.rs", "pub fn foo() {}")
    }).unwrap();
    let library = git::new("bar", |library| {
        library.no_manifest().file("Makefile", "all:")
    }).unwrap();

    let repository = git2::Repository::open(&project.root()).unwrap();
    let url = path2url(library.root()).to_string();
    git::add_submodule(&repository, &url, Path::new("bar"));
    git::commit(&repository);

    let repository = git2::Repository::open(&project.root().join("bar")).unwrap();
    repository
        .reset(
            &repository.revparse_single("HEAD").unwrap(),
            git2::ResetType::Hard,
            None,
        ).unwrap();

    cargo_process("package --no-verify -v")
        .cwd(project.root())
        .with_stderr_contains("[ARCHIVING] bar/Makefile")
        .run();
}

#[test]
fn no_duplicates_from_modified_tracked_files() {
    let root = paths::root().join("all");
    let p = git::repo(&root)
        .file("Cargo.toml", &basic_manifest("foo", "0.0.1"))
        .file("src/main.rs", "fn main() {}")
        .build();
    File::create(p.root().join("src/main.rs"))
        .unwrap()
        .write_all(br#"fn main() { println!("A change!"); }"#)
        .unwrap();
    cargo_process("build").cwd(p.root()).run();
    cargo_process("package --list --allow-dirty")
        .cwd(p.root())
        .with_stdout(
            "\
Cargo.toml
src/main.rs
",
        ).run();
}

#[test]
fn ignore_nested() {
    let cargo_toml = r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
        "#;
    let main_rs = r#"
            fn main() { println!("hello"); }
        "#;
    let p = project()
        .file("Cargo.toml", cargo_toml)
        .file("src/main.rs", main_rs)
        // If a project happens to contain a copy of itself, we should
        // ignore it.
        .file("a_dir/foo/Cargo.toml", cargo_toml)
        .file("a_dir/foo/src/main.rs", main_rs)
        .build();

    p.cargo("package")
        .with_stderr(
            "\
[WARNING] manifest has no documentation[..]
See http://doc.crates.io/manifest.html#package-metadata for more info.
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();
    assert!(p.root().join("target/package/foo-0.0.1.crate").is_file());
    p.cargo("package -l")
        .with_stdout(
            "\
Cargo.toml
src[..]main.rs
",
        ).run();
    p.cargo("package").with_stdout("").run();

    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    for f in ar.entries().unwrap() {
        let f = f.unwrap();
        let fname = f.header().path_bytes();
        let fname = &*fname;
        assert!(
            fname == b"foo-0.0.1/Cargo.toml"
                || fname == b"foo-0.0.1/Cargo.toml.orig"
                || fname == b"foo-0.0.1/src/main.rs",
            "unexpected filename: {:?}",
            f.header().path()
        )
    }
}

#[cfg(unix)] // windows doesn't allow these characters in filenames
#[test]
fn package_weird_characters() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .file("src/:foo", "")
        .build();

    p.cargo("package")
        .with_status(101)
        .with_stderr(
            "\
warning: [..]
See [..]
[PACKAGING] foo [..]
[ERROR] failed to prepare local package for uploading

Caused by:
  cannot package a filename with a special character `:`: src/:foo
",
        ).run();
}

#[test]
fn repackage_on_source_change() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .build();

    p.cargo("package").run();

    // Add another source file
    let mut file = File::create(p.root().join("src").join("foo.rs")).unwrap_or_else(|e| {
        panic!(
            "could not create file {}: {}",
            p.root().join("src/foo.rs").display(),
            e
        )
    });

    file.write_all(br#"fn main() { println!("foo"); }"#)
        .unwrap();
    std::mem::drop(file);

    // Check that cargo rebuilds the tarball
    cargo_process("package")
        .cwd(p.root())
        .with_stderr(
            "\
[WARNING] [..]
See [..]
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();

    // Check that the tarball contains the added file
    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    let entries = ar.entries().unwrap();
    let entry_paths = entries
        .map(|entry| entry.unwrap().path().unwrap().into_owned())
        .collect::<Vec<PathBuf>>();
    assert!(entry_paths.contains(&PathBuf::from("foo-0.0.1/src/foo.rs")));
}

#[test]
#[cfg(unix)]
fn broken_symlink() {
    use std::os::unix::fs;

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = 'foo'
            documentation = 'foo'
            homepage = 'foo'
            repository = 'foo'
        "#,
        ).file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .build();
    t!(fs::symlink("nowhere", &p.root().join("src/foo.rs")));

    p.cargo("package -v")
        .with_status(101)
        .with_stderr_contains(
            "\
error: failed to prepare local package for uploading

Caused by:
  failed to open for archiving: `[..]foo.rs`

Caused by:
  [..]
",
        ).run();
}

#[test]
fn do_not_package_if_repository_is_dirty() {
    let p = project().build();

    // Create a Git repository containing a minimal Rust project.
    let _ = git::repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
        "#,
        ).file("src/main.rs", "fn main() {}")
        .build();

    // Modify Cargo.toml without committing the change.
    p.change_file(
        "Cargo.toml",
        r#"
            [project]
            name = "foo"
            version = "0.0.1"
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
            # change
    "#,
    );

    p.cargo("package")
        .with_status(101)
        .with_stderr(
            "\
error: 1 files in the working directory contain changes that were not yet \
committed into git:

Cargo.toml

to proceed despite this, pass the `--allow-dirty` flag
",
        ).run();
}

#[test]
fn generated_manifest() {
    Package::new("abc", "1.0.0").publish();
    Package::new("def", "1.0.0").alternative(true).publish();
    Package::new("ghi", "1.0.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["alternative-registries"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            exclude = ["*.txt"]
            license = "MIT"
            description = "foo"

            [project.metadata]
            foo = 'bar'

            [workspace]

            [dependencies]
            bar = { path = "bar", version = "0.1" }
            def = { version = "1.0", registry = "alternative" }
            ghi = "1.0"
            abc = "1.0"
        "#,
        ).file("src/main.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.1.0"))
        .file("bar/src/lib.rs", "")
        .build();

    p.cargo("package --no-verify")
        .masquerade_as_nightly_cargo()
        .run();

    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    let mut entry = ar
        .entries()
        .unwrap()
        .map(|f| f.unwrap())
        .find(|e| e.path().unwrap().ends_with("Cargo.toml"))
        .unwrap();
    let mut contents = String::new();
    entry.read_to_string(&mut contents).unwrap();
    // BTreeMap makes the order of dependencies in the generated file deterministic
    // by sorting alphabetically
    assert_eq!(
        &contents[..],
        &*format!(
            r#"# THIS FILE IS AUTOMATICALLY GENERATED BY CARGO
#
# When uploading crates to the registry Cargo will automatically
# "normalize" Cargo.toml files for maximal compatibility
# with all versions of Cargo and also rewrite `path` dependencies
# to registry (e.g. crates.io) dependencies
#
# If you believe there's an error in this file please file an
# issue against the rust-lang/cargo repository. If you're
# editing this file be aware that the upstream Cargo.toml
# will likely look very different (and much more reasonable)

cargo-features = ["alternative-registries"]

[package]
name = "foo"
version = "0.0.1"
authors = []
exclude = ["*.txt"]
description = "foo"
license = "MIT"

[package.metadata]
foo = "bar"
[dependencies.abc]
version = "1.0"

[dependencies.bar]
version = "0.1"

[dependencies.def]
version = "1.0"
registry-index = "{}"

[dependencies.ghi]
version = "1.0"
"#,
            registry::alt_registry()
        )
    );
}

#[test]
fn ignore_workspace_specifier() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"

            authors = []

            [workspace]

            [dependencies]
            bar = { path = "bar", version = "0.1" }
        "#,
        ).file("src/main.rs", "")
        .file(
            "bar/Cargo.toml",
            r#"
            [package]
            name = "bar"
            version = "0.1.0"
            authors = []
            workspace = ".."
        "#,
        ).file("bar/src/lib.rs", "")
        .build();

    p.cargo("package --no-verify")
        .cwd(p.root().join("bar"))
        .run();

    let f = File::open(&p.root().join("target/package/bar-0.1.0.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    let mut entry = ar
        .entries()
        .unwrap()
        .map(|f| f.unwrap())
        .find(|e| e.path().unwrap().ends_with("Cargo.toml"))
        .unwrap();
    let mut contents = String::new();
    entry.read_to_string(&mut contents).unwrap();
    assert_eq!(
        &contents[..],
        r#"# THIS FILE IS AUTOMATICALLY GENERATED BY CARGO
#
# When uploading crates to the registry Cargo will automatically
# "normalize" Cargo.toml files for maximal compatibility
# with all versions of Cargo and also rewrite `path` dependencies
# to registry (e.g. crates.io) dependencies
#
# If you believe there's an error in this file please file an
# issue against the rust-lang/cargo repository. If you're
# editing this file be aware that the upstream Cargo.toml
# will likely look very different (and much more reasonable)

[package]
name = "bar"
version = "0.1.0"
authors = []
"#
    );
}

#[test]
fn package_two_kinds_of_deps() {
    Package::new("other", "1.0.0").publish();
    Package::new("other1", "1.0.0").publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            other = "1.0"
            other1 = { version = "1.0" }
        "#,
        ).file("src/main.rs", "")
        .build();

    p.cargo("package --no-verify").run();
}

#[test]
fn test_edition() {
    if !is_nightly() {
        // --edition is nightly-only
        return;
    }
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["edition"]
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
            edition = "2018"
        "#,
        ).file("src/lib.rs", r#" "#)
        .build();

    p.cargo("build -v").masquerade_as_nightly_cargo()
                // --edition is still in flux and we're not passing -Zunstable-options
                // from Cargo so it will probably error. Only partially match the output
                // until stuff stabilizes
                .with_stderr_contains("\
[COMPILING] foo v0.0.1 ([..])
[RUNNING] `rustc [..]--edition=2018 [..]
").run();
}

#[test]
fn edition_with_metadata() {
    if !is_nightly() {
        // --edition is nightly-only
        return;
    }

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["edition"]
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
            edition = "2018"
            [package.metadata.docs.rs]
            features = ["foobar"]
        "#,
        ).file("src/lib.rs", "")
        .build();

    p.cargo("package").masquerade_as_nightly_cargo().run();
}

#[test]
fn test_edition_missing() {
    // no edition = 2015
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["edition"]
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
        "#,
        ).file("src/lib.rs", r#" "#)
        .build();

    p.cargo("build -v").masquerade_as_nightly_cargo()
                // --edition is still in flux and we're not passing -Zunstable-options
                // from Cargo so it will probably error. Only partially match the output
                // until stuff stabilizes
                .with_stderr_contains("\
[COMPILING] foo v0.0.1 ([..])
[RUNNING] `rustc [..]--edition=2015 [..]
").run();
}

#[test]
fn test_edition_malformed() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["edition"]
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
            edition = "chicken"
        "#,
        ).file("src/lib.rs", r#" "#)
        .build();

    p.cargo("build -v")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr(
            "\
error: failed to parse manifest at `[..]`

Caused by:
  failed to parse the `edition` key

Caused by:
  supported edition values are `2015` or `2018`, but `chicken` is unknown
".to_string(),
        ).run();
}

#[test]
fn test_edition_nightly() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
            edition = "2015"
        "#,
        ).file("src/lib.rs", r#" "#)
        .build();

    p.cargo("build -v")
        .masquerade_as_nightly_cargo()
        .with_status(101)
        .with_stderr(
            "\
error: failed to parse manifest at `[..]`

Caused by:
  editions are unstable

Caused by:
  feature `edition` is required

consider adding `cargo-features = [\"edition\"]` to the manifest
",
        ).run();
}

#[test]
fn package_lockfile() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["publish-lockfile"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish-lockfile = true
        "#,
        ).file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("package")
        .masquerade_as_nightly_cargo()
        .with_stderr(
            "\
[WARNING] manifest has no documentation[..]
See [..]
[PACKAGING] foo v0.0.1 (CWD)
[VERIFYING] foo v0.0.1 (CWD)
[COMPILING] foo v0.0.1 (CWD[..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        ).run();
    assert!(p.root().join("target/package/foo-0.0.1.crate").is_file());
    p.cargo("package -l")
        .masquerade_as_nightly_cargo()
        .with_stdout(
            "\
Cargo.lock
Cargo.toml
src/main.rs
",
        ).run();
    p.cargo("package")
        .masquerade_as_nightly_cargo()
        .with_stdout("")
        .run();

    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    for f in ar.entries().unwrap() {
        let f = f.unwrap();
        let fname = f.header().path_bytes();
        let fname = &*fname;
        assert!(
            fname == b"foo-0.0.1/Cargo.toml"
                || fname == b"foo-0.0.1/Cargo.toml.orig"
                || fname == b"foo-0.0.1/Cargo.lock"
                || fname == b"foo-0.0.1/src/main.rs",
            "unexpected filename: {:?}",
            f.header().path()
        )
    }
}

#[test]
fn package_lockfile_git_repo() {
    let p = project().build();

    // Create a Git repository containing a minimal Rust project.
    let _ = git::repo(&paths::root().join("foo"))
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["publish-lockfile"]

            [project]
            name = "foo"
            version = "0.0.1"
            license = "MIT"
            description = "foo"
            documentation = "foo"
            homepage = "foo"
            repository = "foo"
            publish-lockfile = true
        "#,
        ).file("src/main.rs", "fn main() {}")
        .build();
    p.cargo("package -l")
        .masquerade_as_nightly_cargo()
        .with_stdout(
            "\
.cargo_vcs_info.json
Cargo.lock
Cargo.toml
src/main.rs
",
        ).run();
}

#[test]
fn no_lock_file_with_library() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            cargo-features = ["publish-lockfile"]

            [project]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish-lockfile = true
        "#,
        ).file("src/lib.rs", "")
        .build();

    p.cargo("package").masquerade_as_nightly_cargo().run();

    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    for f in ar.entries().unwrap() {
        let f = f.unwrap();
        let fname = f.header().path().unwrap();
        assert!(!fname.ends_with("Cargo.lock"));
    }
}

#[test]
fn lock_file_and_workspace() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [workspace]
            members = ["foo"]
        "#,
        ).file(
            "foo/Cargo.toml",
            r#"
            cargo-features = ["publish-lockfile"]

            [package]
            name = "foo"
            version = "0.0.1"
            authors = []
            license = "MIT"
            description = "foo"
            publish-lockfile = true
        "#,
        ).file("foo/src/main.rs", "fn main() {}")
        .build();

    p.cargo("package")
        .cwd(p.root().join("foo"))
        .masquerade_as_nightly_cargo()
        .run();

    let f = File::open(&p.root().join("target/package/foo-0.0.1.crate")).unwrap();
    let mut rdr = GzDecoder::new(f);
    let mut contents = Vec::new();
    rdr.read_to_end(&mut contents).unwrap();
    let mut ar = Archive::new(&contents[..]);
    assert!(ar.entries().unwrap().into_iter().any(|f| {
        let f = f.unwrap();
        let fname = f.header().path().unwrap();
        fname.ends_with("Cargo.lock")
    }));
}

#[test]
fn do_not_package_if_src_was_modified() {
    let p = project()
        .file("src/main.rs", r#"fn main() { println!("hello"); }"#)
        .file(
            "build.rs",
            r#"
            use std::fs::File;
            use std::io::Write;

            fn main() {
                let mut file = File::create("src/generated.txt").expect("failed to create file");
                file.write_all(b"Hello, world of generated files.").expect("failed to write");
            }
        "#,
        ).build();

    if cfg!(target_os = "macos") {
        // MacOS has 1s resolution filesystem.
        // If src/main.rs is created within 1s of src/generated.txt, then it
        // won't trigger the modification check.
        sleep_ms(1000);
    }

    p.cargo("package")
        .with_status(101)
        .with_stderr_contains(
            "\
error: failed to verify package tarball

Caused by:
  Source directory was modified by build.rs during cargo publish. \
Build scripts should not modify anything outside of OUT_DIR. Modified file: [..]src/generated.txt

To proceed despite this, pass the `--no-verify` flag.",
        ).run();

    p.cargo("package --no-verify").run();
}
