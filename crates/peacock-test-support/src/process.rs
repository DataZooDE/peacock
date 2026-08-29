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

/// How many ports `spawn` will try before giving up. See its doc comment.
const SPAWN_ATTEMPTS: u32 = 5;

/// A child that exited before becoming ready, with whatever it said on the way
/// out. The stderr is what lets `spawn` tell a lost port race apart from a
/// genuine refusal to boot.
struct SpawnFail {
    status: std::process::ExitStatus,
    stderr: String,
}

/// Conservative match for "the child exited because its TCP bind raced with a
/// concurrent test". Mirrors `triton_tests::stderr_indicates_addr_in_use`.
/// Match narrowly: anything unrecognised must stay a hard failure rather than
/// be retried into a confusing timeout.
fn stderr_indicates_addr_in_use(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("address already in use") || s.contains("addrinuse")
}

impl PeacockProcess {
    /// Spawn `peacock` with `extra_env`, bound to a fresh loopback port, and
    /// wait until `/healthz` answers.
    ///
    /// **Retries a lost port race.** `free_port` binds `127.0.0.1:0`, reads the
    /// number and CLOSES the listener; the child binds it a moment later. Two
    /// tests running concurrently can be handed the same port, and only one
    /// gets it.
    ///
    /// The failure that produces is silent, which is why this is worth the
    /// machinery: the loser's child exits on a bind error, but the port stays
    /// open — held by the WINNER — so `connect` and `/healthz` both SUCCEED and
    /// the caller receives a `PeacockProcess` whose `child` is dead and whose
    /// `addr` points at someone else's server, backed by different fixtures. It
    /// surfaces much later as an assertion about response content, pointing
    /// nowhere near the cause. (Observed in a sibling repo's harness, which had
    /// the identical shape: a scripted-brain test received an answer built from
    /// another test's knowledge base.)
    ///
    /// So poll the CHILD as well as the port — a healthy answer on a port is
    /// not evidence that the answer came from us — and retry on a fresh port
    /// when ours lost. Only `AddrInUse` is retried; any other early exit is a
    /// real bug and panics immediately, with the child's stderr, which the
    /// previous "did not become ready" panic discarded entirely.
    pub async fn spawn(extra_env: HashMap<String, String>) -> Self {
        let mut last_stderr = String::new();
        for attempt in 0..SPAWN_ATTEMPTS {
            match Self::spawn_once(extra_env.clone()).await {
                Ok(p) => return p,
                Err(SpawnFail { status, stderr }) => {
                    if !stderr_indicates_addr_in_use(&stderr) {
                        panic!(
                            "peacock exited early ({status}), not a port race; stderr:\n{stderr}"
                        );
                    }
                    last_stderr = stderr;
                    // Brief backoff lets the OS recycle ephemeral ports.
                    tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt + 1))).await;
                }
            }
        }
        panic!("peacock hit AddrInUse on {SPAWN_ATTEMPTS} ports; last stderr:\n{last_stderr}");
    }

    /// One spawn attempt on one port. `Err` means the child exited before it
    /// became ready; the caller decides whether that was a port race.
    // The happy path moves `child` into `Self` (whose `Drop` waits); the only
    // un-waited paths are the panic and the early-exit return, which reaps.
    #[allow(clippy::zombie_processes)]
    async fn spawn_once(extra_env: HashMap<String, String>) -> Result<Self, SpawnFail> {
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
            // Ask about OUR child first, exactly as `try_spawn` does.
            if let Some(status) = child.try_wait().expect("try_wait") {
                let mut stderr = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut stderr);
                }
                return Err(SpawnFail { status, stderr });
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

    /// Try to spawn; return the child's exit status if it refuses boot quickly
    /// (e.g. a bad manifest, ACC-10). `Ok(process)` if it came up healthy.
    ///
    /// Deliberately does NOT retry: callers use this to assert that a bad
    /// configuration is rejected, so an early exit is the result they want, not
    /// a race to paper over. `spawn` is the retrying variant.
    ///
    /// Shares `spawn_once` with `spawn` so there is one readiness loop rather
    /// than two copies that can drift — which is how the missing `try_wait`
    /// check survived in `spawn` while being present here.
    pub async fn try_spawn(
        extra_env: HashMap<String, String>,
    ) -> Result<Self, std::process::ExitStatus> {
        Self::spawn_once(extra_env).await.map_err(|f| f.status)
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

#[cfg(test)]
mod tests {
    use super::stderr_indicates_addr_in_use;

    /// The real message, captured from `target/debug/peacock` with its bind
    /// address already held by another socket. Pinned as a literal because the
    /// retry in `spawn` is only correct if it actually matches what the binary
    /// prints — a reworded boot error would otherwise turn a port race back
    /// into the silent wrong-server failure this guards against.
    #[test]
    fn the_real_bind_collision_message_is_recognised() {
        assert!(stderr_indicates_addr_in_use(
            "peacock: boot refused: bind 127.0.0.1:37331: Address already in use (os error 98)\n"
        ));
    }

    /// A genuine boot refusal must NOT be retried — it should fail fast with
    /// the reason, not five times with a timeout.
    #[test]
    fn an_ordinary_boot_refusal_is_not_a_port_race() {
        assert!(!stderr_indicates_addr_in_use(
            "peacock: boot refused: no escurel endpoint (set --escurel-url / PEACOCK_ESCUREL_URL or manifest)\n"
        ));
        assert!(!stderr_indicates_addr_in_use(""));
    }
}
