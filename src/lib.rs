use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub const ENV_LINKER: &str = "RLC_LINKER";
pub const ENV_CONCURRENCY: &str = "RLC_CONCURRENCY";
pub const ENV_LOCK_DIR: &str = "RLC_LOCK_DIR";
pub const ENV_VERBOSE: &str = "RLC_VERBOSE";

#[derive(Debug)]
pub struct Config {
    pub linker: OsString,
    pub concurrency: u32,
    pub lock_dir: PathBuf,
    pub verbose: bool,
}

impl Config {
    pub fn from_env() -> io::Result<Self> {
        let linker = env::var_os(ENV_LINKER).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{ENV_LINKER} is not set; point it at the real linker, \
                     e.g. {ENV_LINKER}=clang or {ENV_LINKER}=link.exe"
                ),
            )
        })?;
        Ok(Self {
            linker,
            concurrency: parse_concurrency()?,
            lock_dir: resolve_lock_dir(),
            verbose: verbose_flag(),
        })
    }
}

fn parse_concurrency() -> io::Result<u32> {
    let raw = match env::var(ENV_CONCURRENCY) {
        Ok(s) => s,
        Err(env::VarError::NotPresent) => return Ok(1),
        Err(env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{ENV_CONCURRENCY} is not valid UTF-8"),
            ));
        }
    };
    let n: u32 = raw.trim().parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{ENV_CONCURRENCY} must be a positive integer; got {raw:?}"),
        )
    })?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{ENV_CONCURRENCY} must be >= 1"),
        ));
    }
    Ok(n)
}

fn resolve_lock_dir() -> PathBuf {
    if let Some(d) = env::var_os(ENV_LOCK_DIR) {
        return PathBuf::from(d);
    }
    if let Some(t) = env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(t).join(".rlc-locks");
    }
    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "anon".into());
    env::temp_dir().join(format!("rlc-{user}"))
}

fn verbose_flag() -> bool {
    matches!(env::var(ENV_VERBOSE), Ok(ref s) if !s.is_empty() && s != "0")
}

/// Acquire one of `n` slots rooted at `lock_dir`. The returned file holds the
/// flock; drop it (or let it leave scope) to release.
pub fn acquire_slot(lock_dir: &Path, n: u32, verbose: bool) -> io::Result<File> {
    assert!(n >= 1, "concurrency must be >= 1");
    fs::create_dir_all(lock_dir)?;

    // Offset the sweep by PID so that multiple wrappers don't all stampede slot 0.
    let start = std::process::id() % n;

    for off in 0..n {
        let i = (start + off) % n;
        let f = open_slot(lock_dir, i)?;
        match f.try_lock() {
            Ok(()) => {
                if verbose {
                    eprintln!("[rlc] acquired slot {i} without waiting");
                }
                return Ok(f);
            }
            Err(TryLockError::WouldBlock) => continue,
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }

    if verbose {
        eprintln!("[rlc] all {n} slots busy; blocking on slot {start}");
    }
    let f = open_slot(lock_dir, start)?;
    f.lock()?;
    if verbose {
        eprintln!("[rlc] acquired slot {start} after blocking");
    }
    Ok(f)
}

fn open_slot(dir: &Path, i: u32) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(format!("slot-{i:03}")))
}

/// Read config, acquire a slot, exec the configured linker with our forwarded args,
/// and return its exit code (low byte on Windows; full byte on Unix).
pub fn run() -> io::Result<u8> {
    let cfg = Config::from_env()?;
    let t0 = Instant::now();
    let _slot = acquire_slot(&cfg.lock_dir, cfg.concurrency, cfg.verbose)?;
    let waited = t0.elapsed();
    if cfg.verbose {
        eprintln!("[rlc] waited {waited:?} for slot");
    }
    let t1 = Instant::now();
    let status = Command::new(&cfg.linker)
        .args(env::args_os().skip(1))
        .status()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("failed to invoke linker {:?}: {e}", cfg.linker),
            )
        })?;
    if cfg.verbose {
        eprintln!(
            "[rlc] link finished in {:?} (exit {:?})",
            t1.elapsed(),
            status.code()
        );
    }
    Ok(exit_code(status))
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)) as u8
}

#[cfg(windows)]
fn exit_code(status: std::process::ExitStatus) -> u8 {
    status.code().unwrap_or(1) as u8
}
