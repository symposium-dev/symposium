//! A foreground acquisition must stop when the user interrupts Symposium.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const CHILD_ROOT: &str = "SYMPOSIUM_INSTALL_INTERRUPT_TEST_ROOT";

fn wait_until(mut ready: impl FnMut() -> bool, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Processes {
    parent: Child,
    parent_status: Option<ExitStatus>,
    installer_pid: Option<u32>,
    root: PathBuf,
}

impl Processes {
    fn parent_exited(&mut self) -> bool {
        if self.parent_status.is_none() {
            self.parent_status = self.parent.try_wait().unwrap();
        }
        self.parent_status.is_some()
    }
}

impl Drop for Processes {
    fn drop(&mut self) {
        // Stop the fake installer through its own gate. Unlike signalling its
        // recorded PID, this remains safe if cancellation already killed it
        // and that PID has since been reused.
        let _ = std::fs::write(self.root.join("cleanup"), "");
        if self.parent_status.is_none() {
            unsafe {
                libc::kill(-(self.parent.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = self.parent.wait();
        }
        if let Some(pid) = self.installer_pid {
            wait_until(
                || {
                    self.root.join("installer-stopped").exists()
                        || unsafe { libc::kill(pid as libc::pid_t, 0) } != 0
                },
                Duration::from_secs(5),
            );
        }
    }
}

fn write_gated_installer(root: &Path) -> PathBuf {
    let cargo_script = root.join("mock-cargo");
    // The release gate permits the side effect only after SIGINT. The cleanup
    // gate makes a surviving installer exit without signalling a stale PID.
    // A bounded loop is an additional failsafe if the test process is killed.
    // The INT trap is needed because bash ignores a SIGINT that arrives as its
    // `sleep` exits normally, which would let a signalled installer finish.
    std::fs::write(
        &cargo_script,
        "#!/bin/sh\n\
         case \"$1\" in binstall|install) ;; *) exit 1;; esac\n\
         cd \"$SYMPOSIUM_INSTALL_INTERRUPT_TEST_ROOT\" || exit 1\n\
         trap ': > installer-stopped' EXIT\n\
         trap 'exit 130' INT\n\
         echo $$ > installer-pid\n\
         : > installer-started\n\
         i=0\n\
         while [ ! -f release ]; do\n\
           [ ! -f cleanup ] || exit 0\n\
           i=$((i+1)); [ $i -lt 3000 ] || exit 1\n\
           sleep 0.01\n\
         done\n\
         [ ! -f cleanup ] || exit 0\n\
         : > installer-completed\n\
         i=0\n\
         while [ ! -f cleanup ] && [ $i -lt 3000 ]; do i=$((i+1)); sleep 0.01; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&cargo_script, std::fs::Permissions::from_mode(0o755)).unwrap();
    cargo_script
}

fn assert_interrupt_stops_installer(acquire: bool, test_name: &str) {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        // Passing control: Command::output shares the foreground process group,
        // as installer acquisition did before the cancellation helper changed.
        Command::new(Path::new(&root).join("mock-cargo"))
            .arg("binstall")
            .output()
            .unwrap();
        return;
    }

    let signals = tempfile::tempdir().unwrap();
    let root = signals.path();
    let cargo_script = write_gated_installer(root);
    let command = if acquire {
        let config = root.join("symposium");
        let plugin = config.join("plugins/interrupt-test");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            config.join("config.toml"),
            "auto-update = \"off\"\nauto-sync = false\n[defaults]\nsymposium-recommendations = false\nuser-plugins = true\n",
        )
        .unwrap();
        std::fs::write(
            plugin.join("Symposium.toml"),
            "name = \"interrupt-test\"\ndepends-on = [\"*\"]\n\
             [[installations]]\nname = \"probe\"\nsource = \"cargo\"\n\
             crate = \"interrupt-test-installer\"\nexecutable = \"interrupt-test-installer\"\nglobal = true\n\
             [subcommand.interrupt-probe]\ndescription = \"Test foreground acquisition\"\ncommand = \"probe\"\n",
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-agents"));
        command
            .arg("interrupt-probe")
            .env("SYMPOSIUM_HOME", config)
            .env("SYMPOSIUM_CARGO", &cargo_script);
        command
    } else {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", test_name, "--nocapture"]);
        command
    };
    interrupt_and_assert(command, root, None);
}

fn interrupt_and_assert(mut command: Command, root: &Path, input: Option<&[u8]>) {
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut processes = Processes {
        parent: command
            .env(CHILD_ROOT, root)
            .current_dir(root)
            .process_group(0)
            .spawn()
            .unwrap(),
        parent_status: None,
        installer_pid: None,
        root: root.to_owned(),
    };
    if let Some(input) = input {
        processes
            .parent
            .stdin
            .take()
            .unwrap()
            .write_all(input)
            .unwrap();
    }
    wait_until(
        || root.join("installer-started").exists() || processes.parent_exited(),
        Duration::from_secs(5),
    );
    assert!(
        root.join("installer-started").exists(),
        "the fake installer must actually start"
    );
    let installer_pid: u32 = std::fs::read_to_string(root.join("installer-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    processes.installer_pid = Some(installer_pid);

    // A terminal sends Ctrl-C to the foreground group. The test parent has
    // its own group so this cannot interrupt the surrounding test harness.
    let delivered = unsafe { libc::kill(-(processes.parent.id() as libc::pid_t), libc::SIGINT) };
    let exited = wait_until(|| processes.parent_exited(), Duration::from_secs(5));
    std::fs::write(root.join("release"), "").unwrap();
    let completed = wait_until(
        || root.join("installer-completed").exists(),
        Duration::from_millis(500),
    );

    assert_eq!(delivered, 0, "SIGINT must reach the foreground group");
    assert!(exited, "the interrupted parent must exit");
    let status = processes.parent_status.unwrap();
    assert!(
        status.signal() == Some(libc::SIGINT) || status.code() == Some(130),
        "the parent must exit because of the interrupt"
    );
    assert!(
        !completed,
        "installer continued mutating files after Symposium received Ctrl-C"
    );
}

#[test]
fn ctrl_c_stops_source_acquisition() {
    assert_interrupt_stops_installer(true, "ctrl_c_stops_source_acquisition");
}

#[test]
fn ctrl_c_stops_an_installer_in_the_foreground_group() {
    assert_interrupt_stops_installer(false, "ctrl_c_stops_an_installer_in_the_foreground_group");
}

#[test]
fn ctrl_c_stops_session_start_refresh() {
    let signals = tempfile::tempdir().unwrap();
    let root = signals.path();
    write_gated_installer(root);
    let config = root.join("symposium");
    let plugin = config.join("plugins/interrupt-test");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "auto-update = \"off\"\nauto-sync = true\n[defaults]\nsymposium-recommendations = false\nuser-plugins = true\n",
    )
    .unwrap();
    std::fs::write(
        plugin.join("Symposium.toml"),
        "name = \"interrupt-test\"\ndepends-on = [\"*\"]\n\
         [[installations]]\nname = \"probe\"\nsource = \"cargo\"\n\
         crate = \"interrupt-test-installer\"\nexecutable = \"interrupt-test-installer\"\n\
         git = \"https://example.invalid/installer.git\"\n\
         [subcommand.interrupt-probe]\ndescription = \"Seed the cached installation\"\ncommand = \"probe\"\n\
         [[hooks]]\nname = \"context\"\nevent = \"SessionStart\"\nformat = \"symposium\"\ncommand = \"probe\"\n",
    )
    .unwrap();

    // Seed through the real acquisition path, without depending on its cache
    // key format. On SessionStart the changed remote SHA forces a refresh of
    // that cached installation, which enters the shared interruption gate.
    let cargo_script = root.join("seed-or-refresh-cargo");
    std::fs::write(
        &cargo_script,
        "#!/bin/sh\n\
         case \"$1\" in binstall|install) ;; *) exit 1;; esac\n\
         if [ \"$SYMPOSIUM_INSTALL_INTERRUPT_PREWARM\" = refresh ]; then\n\
           exec \"$SYMPOSIUM_INSTALL_INTERRUPT_TEST_ROOT/mock-cargo\" \"$@\"\n\
         fi\n\
         install_root=\n\
         while [ \"$#\" -gt 0 ]; do\n\
           if [ \"$1\" = --root ]; then shift; install_root=$1; fi\n\
           shift\n\
         done\n\
         [ -n \"$install_root\" ] || exit 1\n\
         mkdir -p \"$install_root/bin\" || exit 1\n\
         printf '#!/bin/sh\\nprintf \"{}\\\\n\"\\n' > \"$install_root/bin/interrupt-test-installer\"\n\
         chmod +x \"$install_root/bin/interrupt-test-installer\"\n",
    )
    .unwrap();
    let git_script = root.join("git");
    std::fs::write(
        &git_script,
        "#!/bin/sh\n\
         [ \"$1\" = ls-remote ] || exit 1\n\
         if [ \"$SYMPOSIUM_INSTALL_INTERRUPT_PREWARM\" = refresh ]; then\n\
           printf 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\\tHEAD\\n'\n\
         else\n\
           printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\tHEAD\\n'\n\
         fi\n",
    )
    .unwrap();
    for script in [&cargo_script, &git_script] {
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(root.to_owned()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )),
    )
    .unwrap();
    let cli = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cargo-agents"));
        command
            .env("SYMPOSIUM_HOME", &config)
            .env("SYMPOSIUM_CARGO", &cargo_script)
            .env(CHILD_ROOT, root)
            .env("PATH", &path)
            .current_dir(root);
        command
    };
    let seeded = cli()
        .arg("interrupt-probe")
        .env("SYMPOSIUM_INSTALL_INTERRUPT_PREWARM", "seed")
        .output()
        .unwrap();
    assert!(
        seeded.status.success(),
        "seeding the installation failed: {}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&seeded.stdout).trim(), "{}");
    assert!(
        !root.join("installer-started").exists(),
        "the interruption gate must be entered by prewarm, not initial installation"
    );

    let mut command = cli();
    command
        .args(["hook", "claude", "session-start"])
        .env("SYMPOSIUM_INSTALL_INTERRUPT_PREWARM", "refresh");
    let payload = serde_json::json!({
        "hook_event_name": "SessionStart",
        "cwd": root,
    })
    .to_string();
    interrupt_and_assert(command, root, Some(payload.as_bytes()));
}
