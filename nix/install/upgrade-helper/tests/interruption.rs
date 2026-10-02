use sha2::{Digest, Sha256};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn actual_term_signal_rolls_back_before_child_exits() {
    let root = std::env::temp_dir().join(format!("upgrade-interruption-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let result = std::panic::catch_unwind(|| {
        for (name, bytes) in [
            ("flake.nix", "old-flake"),
            ("flake.lock", "old-lock"),
            ("nixfied.nix", "owned"),
            ("new-flake", "new-flake"),
            ("new-lock", "new-lock"),
        ] {
            fs::write(root.join(name), bytes).unwrap();
        }
        let hashes =
            ["old-flake", "old-lock", "owned"].map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        let log = root.join("stderr");
        let mut child = Command::new(env!("CARGO_BIN_EXE_nixfied-upgrade-files"))
            .arg("apply")
            .arg(&root)
            .args(&hashes)
            .arg(root.join("new-flake"))
            .arg(root.join("new-lock"))
            .env("NIXFIED_UPGRADE_TEST_PAUSE_AFTER_LOCK", "10")
            .stdout(Stdio::null())
            .stderr(fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fs::read_to_string(&log)
            .unwrap()
            .contains("test pause after lock apply")
        {
            if child.try_wait().unwrap().is_some() || Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("child did not reach apply pause");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            fs::read_to_string(root.join("flake.lock")).unwrap(),
            "new-lock"
        );
        assert_eq!(
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("interrupted child did not exit");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(130));
        assert!(
            fs::read_to_string(log)
                .unwrap()
                .contains("upgrade interrupted; candidate rolled back")
        );
        assert_eq!(
            fs::read_to_string(root.join("flake.nix")).unwrap(),
            "old-flake"
        );
        assert_eq!(
            fs::read_to_string(root.join("flake.lock")).unwrap(),
            "old-lock"
        );
        assert_eq!(
            fs::read_to_string(root.join("nixfied.nix")).unwrap(),
            "owned"
        );
        assert!(!fs::read_dir(&root).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".nixfied-upgrade.")
        }));
    });
    fs::remove_dir_all(&root).unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
