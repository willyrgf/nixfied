use super::*;

#[test]
fn source_edits_only_selected_literal_and_preserves_all_other_bytes() {
    assert_eq!(
        rewrite("{ inputs.nixfied.url = \"old\"; }", "path:a${x}\"\\\n").unwrap(),
        "{ inputs.nixfied.url = \"path:a\\${x}\\\"\\\\\\n\"; }"
    );
    for source in [
        "{ inputs.nixfied.url = \"old\"; # } comment\n outputs = _: { x = \"}\"; }; }",
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
        for (name, contents) in [("flake.nix", "old-flake"), ("flake.lock", "old-lock")] {
            fs::write(root.join(name), contents).unwrap();
            fs::write(candidates.join(name), format!("new-{contents}")).unwrap();
        }
        fs::write(root.join("nixfied.nix"), "owned").unwrap();
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
