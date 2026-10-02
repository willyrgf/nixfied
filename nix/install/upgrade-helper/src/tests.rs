use super::*;

#[test]
fn source_edits_only_selected_literal_and_preserves_all_other_bytes() {
    for source in [
        "{ inputs.nixfied.url = \"old\"; # } comment\n outputs = _: { x = \"}\"; }; }",
        "{ inputs = { nixfied.url=\"old\"; }; outputs = _: {}; }",
        "{ inputs.nixfied = { url = \"old\"; inputs.nixpkgs.follows=\"nixpkgs\"; }; }",
        "{ inputs = { nixfied = { url = \"old\"; }; }; }",
        "{ inputs.\"nixfied\".url = \"old\"; description = ''\n { inputs.nixfied.url = \"decoy\"; }\n ''; }",
    ] {
        assert_eq!(
            rewrite(source, "new").unwrap(),
            source.replacen("\"old\"", "\"new\"", 1)
        );
    }
}
#[test]
fn source_rejects_ambiguous_or_computed_input_without_guessing() {
    for source in [
        "{ inputs.nixfied.url = \"a\"; inputs.nixfied.url = \"b\"; }",
        "{ inputs.nixfied.url = \"${value}\"; }",
        "{ inputs.nixfied.url = base + \"suffix\"; }",
        "let x = {}; in x",
        "{ inputs = makeInputs {}; }",
        "{ inputs.nixfied = builtins.fromJSON json; }",
        "{ outputs = _: { inputs.nixfied.url = \"decoy\"; }; }",
        "{ inputs.nixfied.url = \"a\";",
    ] {
        assert!(rewrite(source, "new").is_err(), "accepted {source}");
    }
}
#[test]
fn source_escapes_interpolation_quotes_and_controls_as_literal_bytes() {
    let url = "path:a${danger}\"\\\n\r\t";
    let result = rewrite("{ inputs.nixfied.url = \"old\"; }", url).unwrap();
    let parsed = rnix::Root::parse(&result);
    assert!(parsed.errors().is_empty());
    let ast::Expr::AttrSet(root) = parsed.tree().expr().unwrap() else {
        panic!()
    };
    let mut found = Vec::new();
    find_url(root, &[], &mut found).unwrap();
    assert_eq!(literal(&found[0]).unwrap(), url);
}
struct Fixture {
    root: PathBuf,
    candidates: PathBuf,
    hashes: [String; 3],
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "upgrade-files-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let candidates = root.join("candidates");
        fs::create_dir(&candidates).unwrap();
        for (name, contents) in [
            ("flake.nix", "old-flake"),
            ("flake.lock", "old-lock"),
            ("nixfied.nix", "owned"),
        ] {
            fs::write(root.join(name), contents).unwrap();
            fs::write(candidates.join(name), format!("new-{contents}")).unwrap();
        }
        let hashes = ["flake.nix", "flake.lock", "nixfied.nix"]
            .map(|name| read(&root.join(name)).unwrap().unwrap().hash());
        Self {
            root,
            candidates,
            hashes,
        }
    }
    fn apply(&self, hook: impl FnMut(Point, &Path) -> Result<()>) -> Result<()> {
        apply(
            &self.root,
            [&self.hashes[0], &self.hashes[1], &self.hashes[2]],
            Some(&self.candidates.join("flake.nix")),
            Some(&self.candidates.join("flake.lock")),
            hook,
        )
    }
    fn contents(&self, name: &str) -> String {
        fs::read_to_string(self.root.join(name)).unwrap()
    }
    fn temps(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .as_bytes()
                    .starts_with(b".nixfied-upgrade.")
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn applies_both_files_preserving_project_and_modes_and_removing_temps() {
    let fixture = Fixture::new();
    fs::set_permissions(
        fixture.root.join("flake.nix"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    fixture.apply(|_, _| Ok(())).unwrap();
    assert_eq!(fixture.contents("flake.nix"), "new-old-flake");
    assert_eq!(fixture.contents("flake.lock"), "new-old-lock");
    assert_eq!(fixture.contents("nixfied.nix"), "owned");
    assert_eq!(
        fs::metadata(fixture.root.join("flake.nix")).unwrap().mode() & 0o777,
        0o640
    );
    assert!(fixture.temps().is_empty());
}
#[test]
fn stale_original_and_symlink_fail_before_mutation() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("flake.nix"), "edited").unwrap();
    assert!(matches!(
        fixture.apply(|_, _| Ok(())),
        Err(Failure::Conflict)
    ));
    assert_eq!(fixture.contents("flake.lock"), "old-lock");
    fs::remove_file(fixture.root.join("flake.nix")).unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("nixfied.nix"),
        fixture.root.join("flake.nix"),
    )
    .unwrap();
    assert!(matches!(
        fixture.apply(|_, _| Ok(())),
        Err(Failure::Operation(_))
    ));
    assert_eq!(fixture.contents("nixfied.nix"), "owned");
    assert!(fixture.temps().is_empty());
}
#[test]
fn failure_or_interruption_between_writes_restores_original_bytes() {
    for interrupted in [false, true] {
        let fixture = Fixture::new();
        let result = fixture.apply(|point, _| {
            if point == Point::AfterLock {
                if interrupted {
                    Err(Failure::Interrupted)
                } else {
                    Err(io::Error::other("injected failure").into())
                }
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(fixture.contents("flake.nix"), "old-flake");
        assert_eq!(fixture.contents("flake.lock"), "old-lock");
        assert!(fixture.temps().is_empty());
    }
}
#[test]
fn edit_between_writes_is_preserved_and_prior_write_is_rolled_back() {
    let fixture = Fixture::new();
    let result = fixture.apply(|point, root| {
        if point == Point::AfterLock {
            fs::write(root.join("flake.nix"), "editor").unwrap();
        }
        Ok(())
    });
    assert!(matches!(result, Err(Failure::Conflict)));
    assert_eq!(fixture.contents("flake.nix"), "editor");
    assert_eq!(fixture.contents("flake.lock"), "old-lock");
    assert!(fixture.temps().is_empty());
}
#[test]
fn edit_of_applied_file_is_not_overwritten_during_rollback() {
    let fixture = Fixture::new();
    let result = fixture.apply(|point, path| {
        if point == Point::AfterExchange {
            fs::write(path, "editor").unwrap();
            return Err(Failure::Conflict);
        }
        Ok(())
    });
    let Err(Failure::Rollback(paths)) = result else {
        panic!("expected rollback refusal");
    };
    assert_eq!(fixture.contents("flake.lock"), "editor");
    assert_eq!(fixture.contents("flake.nix"), "old-flake");
    assert_eq!(paths.len(), 1);
    assert_eq!(fs::read_to_string(&paths[0]).unwrap(), "old-lock");
}
#[test]
fn replacement_inode_with_identical_bytes_is_not_overwritten_during_rollback() {
    let fixture = Fixture::new();
    let result = fixture.apply(|point, path| {
        if point == Point::AfterExchange {
            let other = path.with_extension("editor");
            fs::write(&other, fs::read(path).unwrap()).unwrap();
            fs::rename(&other, path).unwrap();
            return Err(Failure::Conflict);
        }
        Ok(())
    });
    assert!(matches!(result, Err(Failure::Rollback(_))));
    assert_eq!(fixture.contents("flake.lock"), "new-old-lock");
    assert_eq!(fixture.temps().len(), 1);
}
#[test]
fn noop_keeps_inode_and_missing_lock_is_allowed_for_url_only() {
    let fixture = Fixture::new();
    fs::copy(
        fixture.root.join("flake.nix"),
        fixture.candidates.join("flake.nix"),
    )
    .unwrap();
    fs::remove_file(fixture.root.join("flake.lock")).unwrap();
    let before = read(&fixture.root.join("flake.nix")).unwrap().unwrap();
    apply(
        &fixture.root,
        [&fixture.hashes[0], "absent", &fixture.hashes[2]],
        Some(&fixture.candidates.join("flake.nix")),
        None,
        |_, _| Ok(()),
    )
    .unwrap();
    assert!(unchanged(&fixture.root.join("flake.nix"), &before));
    assert!(fixture.temps().is_empty());
}
#[test]
fn directory_lock_serializes_another_open_description() {
    let fixture = Fixture::new();
    let _lock = DirectoryLock::acquire(&fixture.root).unwrap();
    let second = File::open(&fixture.root).unwrap();
    assert_ne!(
        unsafe { libc::flock(second.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert_eq!(io::Error::last_os_error().kind(), io::ErrorKind::WouldBlock);
}

#[test]
fn project_owned_declaration_symlink_is_observed_but_never_replaced() {
    let fixture = Fixture::new();
    fs::rename(
        fixture.root.join("nixfied.nix"),
        fixture.root.join("declaration.nix"),
    )
    .unwrap();
    std::os::unix::fs::symlink("declaration.nix", fixture.root.join("nixfied.nix")).unwrap();
    fixture.apply(|_, _| Ok(())).unwrap();
    assert_eq!(
        fs::read_link(fixture.root.join("nixfied.nix")).unwrap(),
        Path::new("declaration.nix")
    );
    assert_eq!(fixture.contents("declaration.nix"), "owned");
}
