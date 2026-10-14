//! Isolate the subprocess chains used to verify preference persistence.
//!
//! Linux/Windows preferences rewrite one file, with only an in-process lock. Parallel
//! restart tests must use different config directories even when they use different keys.
use std::process::Command;

pub(crate) struct PreferenceTest {
    #[cfg(target_os = "macos")]
    key: &'static str,
    #[cfg(target_os = "macos")]
    saved: Option<String>,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    directory: std::path::PathBuf,
}

impl PreferenceTest {
    pub(crate) fn new(key: &'static str) -> Self {
        #[cfg(target_os = "macos")]
        {
            // NSUserDefaults merges writes to separate keys. The test executable has
            // its own domain; preserve that domain's previous value for this test key.
            let saved = day::prefs::get(key);
            day::prefs::remove(key);
            Self { key, saved }
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let directory = std::env::temp_dir().join(format!(
                    "day-news-prefs-{key}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&directory) {
                    Ok(()) => return Self { directory },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create isolated preference directory: {error}"),
                }
            }
        }
    }

    pub(crate) fn child(&self, test: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", test]);
        // Override only child environments, never the multithreaded test runner's.
        // All restarts in one test retain the same private store.
        #[cfg(target_os = "linux")]
        command.env("XDG_CONFIG_HOME", &self.directory);
        #[cfg(target_os = "windows")]
        command.env("APPDATA", &self.directory);
        command
    }
}

impl Drop for PreferenceTest {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(value) = &self.saved {
            day::prefs::set(self.key, value);
        } else {
            day::prefs::remove(self.key);
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
