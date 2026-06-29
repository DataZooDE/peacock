//! `PeacockProcess` — spawn the real `peacock` binary for no-mock lifecycle /
//! observability tests (FR-L, FR-O). Mirrors escurel's `EscurelProcess` and
//! Triton's `TritonProcess`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A running `peacock` binary bound to a loopback port.
pub struct PeacockProcess {
    child: Child,
    addr: std::net::SocketAddr,
}

fn binary_path() -> PathBuf {
    // crates/peacock-test-support → workspace root → target/debug/peacock.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/peacock")
        .canonicalize()
        .expect("peacock binary built at target/debug/peacock (run `cargo build`)")
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

impl PeacockProcess {
    /// Spawn `peacock` with `extra_env`, bound to a fresh loopback port, and
    /// wait until `/healthz` answers. Panics if it does not become ready.
    // The happy path moves `child` into `Self` (whose `Drop` waits); the only
    // un-waited path is the panic, which fails the test anyway.
    #[allow(clippy::zombie_processes)]
    pub async fn spawn(extra_env: HashMap<String, String>) -> Self {
        let port = free_port();
        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        let mut cmd = Command::new(binary_path());
        cmd.env("PEACOCK_BIND", addr.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().expect("spawn peacock");

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok()
                && reqwest_ok(&format!("http://{addr}/healthz")).await
            {
                return Self { child, addr };
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("peacock did not become ready on {addr}");
    }

    /// Try to spawn; return the child's exit status if it refuses boot quickly
    /// (e.g. a bad manifest, ACC-10). `Ok(process)` if it came up healthy.
    #[allow(clippy::zombie_processes)]
    pub async fn try_spawn(
        extra_env: HashMap<String, String>,
    ) -> Result<Self, std::process::ExitStatus> {
        let port = free_port();
        let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let mut cmd = Command::new(binary_path());
        cmd.env("PEACOCK_BIND", addr.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn peacock");

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                return Err(status); // exited before becoming ready (boot refused)
            }
            if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok()
                && reqwest_ok(&format!("http://{addr}/healthz")).await
            {
                return Ok(Self { child, addr });
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("peacock neither came up nor exited on {addr}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The base URL (`http://127.0.0.1:<port>`).
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Send SIGTERM and wait for graceful exit; returns the exit status
    /// (expected success, FR-L-2).
    pub fn terminate(mut self) -> std::process::ExitStatus {
        let pid = self.child.id();
        // SIGTERM via `kill` to exercise the binary's real signal handler.
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                return self.child.wait().expect("wait");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for PeacockProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn reqwest_ok(url: &str) -> bool {
    // A tiny dependency-free GET via std TCP would be more code; reqwest is
    // already in the test dependency graph.
    reqwest::get(url)
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
