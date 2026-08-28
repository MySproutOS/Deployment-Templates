use std::{env, fs, path::PathBuf, process::Command, thread, time::Duration};

const CHILD_ARGUMENT: &str = "--sprout-isolation-descendant";

fn main() {
    if env::args().any(|argument| argument == CHILD_ARGUMENT) {
        descendant();
        return;
    }

    let executable = executable_path();
    let mut child = Command::new(executable)
        .arg(CHILD_ARGUMENT)
        .spawn()
        .expect("probe descendant must spawn");

    #[cfg(feature = "smoke")]
    {
        assert!(child.wait().expect("smoke descendant must exit").success());
        assert_eq!(
            fs::read("descendant-smoke").expect("smoke marker must be readable"),
            b"spawned"
        );
        fs::remove_file("descendant-smoke").expect("smoke marker must self-clean");
    }
    #[cfg(not(feature = "smoke"))]
    {
        thread::sleep(Duration::from_secs(131));
        let _ = child.wait();
    }
}

fn executable_path() -> PathBuf {
    #[cfg(all(target_os = "linux", not(feature = "smoke")))]
    {
        // NativeIsolationProvider deliberately exposes no /proc; /plugin is its fixed read-only
        // mount for the verified executable.
        PathBuf::from("/plugin")
    }
    #[cfg(not(all(target_os = "linux", not(feature = "smoke"))))]
    {
        env::current_exe().expect("current proof executable must resolve")
    }
}

fn descendant() {
    #[cfg(feature = "smoke")]
    {
        thread::sleep(Duration::from_millis(50));
        fs::write("descendant-smoke", b"spawned").expect("smoke descendant marker must write");
    }
    #[cfg(not(feature = "smoke"))]
    {
        thread::sleep(Duration::from_secs(130));
        fs::write(
            "descendant-survived",
            b"sandbox failed to kill process tree",
        )
        .expect("surviving descendant must expose containment failure");
        thread::sleep(Duration::from_secs(1));
    }
}
