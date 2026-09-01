// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Keeps an unreachable network mount from stalling the rest of the app.
//!
//! A remote filesystem whose server stopped answering (a phone asleep behind an sshfs, a
//! share behind a VPN that dropped) does not fail: every `stat`, `statvfs` or `readdir`
//! that reaches it blocks until the transport gives up, tens of seconds per call. Listing
//! the *parent* directory is enough to hit it, so a picker opening in `$HOME` froze for as
//! long as the phone was off.
//!
//! Two kernel facts make the mitigation cheap. The mount table (`/proc/self/mountinfo`)
//! says which paths are mount points of a remote filesystem without touching them, and
//! `statx(AT_STATX_DONT_SYNC)` hands back the attributes the kernel already holds for a
//! mount point without a round trip to the server. So the app never stats a remote mount
//! point synchronously: it takes the cached attributes and asks this registry how the mount
//! is doing.
//!
//! The registry probes each remote mount with a `statvfs` on a detached thread. A probe
//! that has not answered within [`PROBE_DEADLINE`] marks the mount unresponsive; the thread
//! is left to finish on its own (it will, once the transport times out) and its late answer
//! flips the mount back. At most one probe per mount is ever in flight, so a dead mount
//! costs one parked thread, not one per poll. Every state change is broadcast to the
//! webviews as a [`MOUNT_HEALTH_CHANGED_EVENT`].
//!
//! Only Linux reads the mount table today; elsewhere every query answers "not a remote
//! mount" and behavior is unchanged.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// How long a probe may take before the mount counts as unresponsive. Generous enough for a
/// healthy share on the far side of a VPN, short enough that a listing never looks stuck.
pub const PROBE_DEADLINE: Duration = Duration::from_millis(1500);
/// A gate waits this long for a first verdict before letting the caller through or not.
pub const GATE_WAIT: Duration = Duration::from_millis(1700);
const RESPONSIVE_RECHECK: Duration = Duration::from_secs(5);
const UNRESPONSIVE_RECHECK: Duration = Duration::from_secs(15);
const MOUNT_TABLE_TTL: Duration = Duration::from_secs(1);
const HEALTH_POLL_STEP: Duration = Duration::from_millis(25);
pub const MOUNT_HEALTH_CHANGED_EVENT: &str = "mount-health-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountHealth {
    Responsive,
    Unresponsive,
    /// No verdict yet: the first probe is still within its deadline.
    Probing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMount {
    pub mount_point: PathBuf,
    pub file_system: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MountStorage {
    pub total_space: u64,
    pub available_space: u64,
    pub is_read_only: bool,
}

/// What the kernel knows about a path without asking the filesystem behind it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CachedAttributes {
    pub is_dir: bool,
    pub modified_time: u64,
    pub accessed_time: u64,
    pub created_time: u64,
}

#[derive(Debug, Clone, Serialize)]
struct MountHealthChangedPayload {
    mount_point: String,
    health: MountHealth,
}

struct MountTable {
    mounts: Vec<NetworkMount>,
    by_mount_point: HashMap<PathBuf, usize>,
    read_at: Instant,
}

#[derive(Default)]
struct MountRecord {
    /// Last verdict and when it landed.
    settled: Option<(MountHealth, Instant)>,
    /// Numbers from the last probe that answered, kept while the mount is down so the drive
    /// list can still show a size.
    storage: Option<MountStorage>,
    /// Set while a probe is in flight; an overdue one reads as unresponsive.
    probe_started: Option<Instant>,
    /// Last state broadcast to the webviews, to emit only real changes.
    reported: Option<MountHealth>,
}

static MOUNT_TABLE: Mutex<Option<Arc<MountTable>>> = Mutex::new(None);
static RECORDS: OnceLock<Mutex<HashMap<PathBuf, MountRecord>>> = OnceLock::new();
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn install_app_handle(app_handle: AppHandle) {
    let _ = APP_HANDLE.set(app_handle);
}

fn records() -> &'static Mutex<HashMap<PathBuf, MountRecord>> {
    RECORDS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Filesystems whose calls go over a transport that can stop answering. MTP filesystems are
/// listed too: a phone that goes to sleep on the USB cable stalls exactly like a remote one.
pub fn is_network_filesystem(file_system: &str) -> bool {
    matches!(
        file_system.to_ascii_lowercase().as_str(),
        "nfs"
            | "nfs4"
            | "cifs"
            | "smb3"
            | "smbfs"
            | "ncpfs"
            | "afs"
            | "afpfs"
            | "ceph"
            | "glusterfs"
            | "9p"
            | "davfs"
            | "fuse.sshfs"
            | "fuse.rclone"
            | "fuse.gvfsd-fuse"
            | "fuse.s3fs"
            | "fuse.davfs2"
            | "fuse.curlftpfs"
            | "fuse.jmtpfs"
            | "fuse.simple-mtpfs"
            | "fuse.go-mtpfs"
            | "fuse.mtpfs"
    )
}

// ---------------------------------------------------------------------------
// Mount table
// ---------------------------------------------------------------------------

impl MountTable {
    fn from_mounts(mounts: Vec<NetworkMount>) -> Self {
        let by_mount_point = mounts
            .iter()
            .enumerate()
            .map(|(index, mount)| (mount.mount_point.clone(), index))
            .collect();

        Self {
            mounts,
            by_mount_point,
            read_at: Instant::now(),
        }
    }

    fn read() -> Self {
        #[cfg(target_os = "linux")]
        {
            let contents = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
            Self::from_mounts(parse_mountinfo(&contents))
        }

        #[cfg(not(target_os = "linux"))]
        {
            Self::from_mounts(Vec::new())
        }
    }
}

fn mount_table() -> Arc<MountTable> {
    let mut cached = MOUNT_TABLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(table) = cached.as_ref() {
        if table.read_at.elapsed() < MOUNT_TABLE_TTL {
            return Arc::clone(table);
        }
    }

    let table = Arc::new(MountTable::read());
    *cached = Some(Arc::clone(&table));
    table
}

/// One `/proc/self/mountinfo` line:
/// `36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue`.
/// The mount point is the fifth field; the filesystem type and source follow the `-`
/// separator. Spaces and other awkward bytes in paths are octal-escaped.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn parse_mountinfo(contents: &str) -> Vec<NetworkMount> {
    contents
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let mount_point = fields.nth(4)?;
            let mut after_separator = fields.skip_while(|field| *field != "-").skip(1);
            let file_system = after_separator.next()?;
            let source = after_separator.next().unwrap_or_default();

            if !is_network_filesystem(file_system) {
                return None;
            }

            Some(NetworkMount {
                mount_point: PathBuf::from(unescape_mountinfo(mount_point)),
                file_system: file_system.to_string(),
                source: unescape_mountinfo(source),
            })
        })
        .collect()
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn unescape_mountinfo(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &field[index + 1..index + 4];
            if let Ok(value) = u8::from_str_radix(octal, 8) {
                output.push(value);
                index += 4;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

/// The remote mount whose mount point is exactly `path`.
pub fn network_mount_at(path: &Path) -> Option<NetworkMount> {
    let table = mount_table();
    table
        .by_mount_point
        .get(path)
        .map(|&index| table.mounts[index].clone())
}

/// The innermost remote mount that `path` lives on, if any.
pub fn network_mount_containing(path: &Path) -> Option<NetworkMount> {
    let table = mount_table();
    table
        .mounts
        .iter()
        .filter(|mount| path.starts_with(&mount.mount_point))
        .max_by_key(|mount| mount.mount_point.as_os_str().len())
        .cloned()
}

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

/// Current verdict for a mount, never blocking. Kicks off a probe when one is due.
pub fn health_of(mount: &NetworkMount) -> MountHealth {
    ensure_probe(mount);

    let records = records()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(record) = records.get(&mount.mount_point) else {
        return MountHealth::Probing;
    };

    if let Some(started) = record.probe_started {
        if started.elapsed() >= PROBE_DEADLINE {
            return MountHealth::Unresponsive;
        }
    }

    record
        .settled
        .map(|(health, _)| health)
        .unwrap_or(MountHealth::Probing)
}

/// Like [`health_of`], but gives a first probe up to `max_wait` to answer instead of
/// reporting `Probing`.
pub fn wait_for_health(mount: &NetworkMount, max_wait: Duration) -> MountHealth {
    let deadline = Instant::now() + max_wait;

    loop {
        let health = health_of(mount);
        if health != MountHealth::Probing || Instant::now() >= deadline {
            return health;
        }
        std::thread::sleep(HEALTH_POLL_STEP);
    }
}

/// Last known size figures for a mount, possibly stale while it is down.
pub fn storage_of(mount: &NetworkMount) -> Option<MountStorage> {
    records()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&mount.mount_point)
        .and_then(|record| record.storage)
}

/// True when `path` lives on a remote mount that is known not to answer.
pub fn is_unresponsive_path(path: &Path) -> bool {
    network_mount_containing(path)
        .map(|mount| wait_for_health(&mount, GATE_WAIT) == MountHealth::Unresponsive)
        .unwrap_or(false)
}

/// True when `path` is itself the mount point of a remote mount that does not answer.
pub fn is_unresponsive_mount_point(path: &Path) -> bool {
    network_mount_at(path)
        .map(|mount| wait_for_health(&mount, GATE_WAIT) == MountHealth::Unresponsive)
        .unwrap_or(false)
}

/// The check every blocking operation on user-supplied paths runs first.
pub fn ensure_responsive(path: &Path) -> Result<(), String> {
    match network_mount_containing(path) {
        Some(mount) if wait_for_health(&mount, GATE_WAIT) == MountHealth::Unresponsive => {
            Err(unresponsive_error(&mount))
        }
        _ => Ok(()),
    }
}

pub fn unresponsive_error(mount: &NetworkMount) -> String {
    format!(
        "Storage at {} is not responding",
        mount.mount_point.display()
    )
}

fn ensure_probe(mount: &NetworkMount) {
    {
        let mut records = records()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = records.entry(mount.mount_point.clone()).or_default();

        if record.probe_started.is_some() {
            return;
        }

        let due = match record.settled {
            None | Some((MountHealth::Probing, _)) => true,
            Some((MountHealth::Responsive, at)) => at.elapsed() >= RESPONSIVE_RECHECK,
            Some((MountHealth::Unresponsive, at)) => at.elapsed() >= UNRESPONSIVE_RECHECK,
        };

        if !due {
            return;
        }

        record.probe_started = Some(Instant::now());
    }

    spawn_probe(mount.mount_point.clone());
}

fn spawn_probe(mount_point: PathBuf) {
    let probed_mount_point = mount_point.clone();
    let spawned = std::thread::Builder::new()
        .name("mount-probe".to_string())
        .spawn(move || {
            let mount_point = probed_mount_point;
            let (sender, receiver) = std::sync::mpsc::channel();
            let probe_path = mount_point.clone();
            let worker = std::thread::Builder::new()
                .name("mount-probe-statvfs".to_string())
                .spawn(move || {
                    let _ = sender.send(statvfs_storage(&probe_path));
                });

            if worker.is_err() {
                settle(&mount_point, None);
                return;
            }

            match receiver.recv_timeout(PROBE_DEADLINE) {
                Ok(result) => settle(&mount_point, result),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    report(&mount_point, MountHealth::Unresponsive);
                    // The worker is parked in the kernel; it returns when the transport
                    // gives up, and that late answer is still the truth about the mount.
                    let late = receiver.recv().ok().flatten();
                    settle(&mount_point, late);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => settle(&mount_point, None),
            }
        });

    if spawned.is_err() {
        let mut records = records()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(record) = records.get_mut(&mount_point) {
            record.probe_started = None;
        }
    }
}

fn settle(mount_point: &Path, storage: Option<MountStorage>) {
    let health = if storage.is_some() {
        MountHealth::Responsive
    } else {
        MountHealth::Unresponsive
    };

    {
        let mut records = records()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = records.entry(mount_point.to_path_buf()).or_default();
        record.probe_started = None;
        record.settled = Some((health, Instant::now()));
        if storage.is_some() {
            record.storage = storage;
        }
    }

    report(mount_point, health);
}

fn report(mount_point: &Path, health: MountHealth) {
    let changed = {
        let mut records = records()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = records.entry(mount_point.to_path_buf()).or_default();
        if record.reported == Some(health) {
            false
        } else {
            record.reported = Some(health);
            true
        }
    };

    if !changed {
        return;
    }

    if let Some(app_handle) = APP_HANDLE.get() {
        let payload = MountHealthChangedPayload {
            mount_point: crate::utils::normalize_path(&mount_point.to_string_lossy()),
            health,
        };
        if let Err(error) = app_handle.emit(MOUNT_HEALTH_CHANGED_EVENT, payload) {
            log::warn!("Failed to emit {MOUNT_HEALTH_CHANGED_EVENT}: {error}");
        }
    }
}

// ---------------------------------------------------------------------------
// Kernel calls
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn statvfs_storage(path: &Path) -> Option<MountStorage> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buffer = std::mem::MaybeUninit::<libc::statvfs>::uninit();

    // SAFETY: `c_path` is a valid NUL-terminated string and `buffer` is a properly sized,
    // writable `statvfs` that is only read after the call reports success.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), buffer.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }

    let stat = unsafe { buffer.assume_init() };
    let fragment_size = stat.f_frsize as u64;

    Some(MountStorage {
        total_space: fragment_size.saturating_mul(stat.f_blocks as u64),
        available_space: fragment_size.saturating_mul(stat.f_bavail as u64),
        is_read_only: (stat.f_flag as u64 & libc::ST_RDONLY as u64) != 0,
    })
}

#[cfg(not(unix))]
fn statvfs_storage(_path: &Path) -> Option<MountStorage> {
    None
}

/// Attributes for `path` from the kernel's cache, never asking the filesystem behind it.
/// `None` when the platform has no such call or the kernel holds nothing for the path.
#[cfg(target_os = "linux")]
pub fn cached_attributes(path: &Path) -> Option<CachedAttributes> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut buffer = std::mem::MaybeUninit::<libc::statx>::uninit();

    // Via the raw syscall rather than the libc wrapper, which some libcs do not export.
    // SAFETY: arguments follow the statx(2) contract; the buffer is only read on success.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_statx,
            libc::AT_FDCWD as libc::c_long,
            c_path.as_ptr(),
            (libc::AT_STATX_DONT_SYNC | libc::AT_SYMLINK_NOFOLLOW) as libc::c_long,
            (libc::STATX_BASIC_STATS | libc::STATX_BTIME) as libc::c_long,
            buffer.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return None;
    }

    let stat = unsafe { buffer.assume_init() };
    let to_unix_ms = |timestamp: libc::statx_timestamp| -> u64 {
        if timestamp.tv_sec < 0 {
            0
        } else {
            (timestamp.tv_sec as u64).saturating_mul(1000) + (timestamp.tv_nsec as u64) / 1_000_000
        }
    };
    let has_birth_time = stat.stx_mask & libc::STATX_BTIME != 0;

    Some(CachedAttributes {
        is_dir: (stat.stx_mode as u32 & libc::S_IFMT) == libc::S_IFDIR,
        modified_time: to_unix_ms(stat.stx_mtime),
        accessed_time: to_unix_ms(stat.stx_atime),
        created_time: if has_birth_time {
            to_unix_ms(stat.stx_btime)
        } else {
            0
        },
    })
}

#[cfg(not(target_os = "linux"))]
pub fn cached_attributes(_path: &Path) -> Option<CachedAttributes> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
25 1 0:22 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw
101 25 0:57 / /mnt/somewhere rw,relatime shared:60 - cifs //somewhere/zero rw,vers=3.1.1
102 101 0:58 / /mnt/somewhere/My\\040Documents rw,relatime shared:61 - cifs //somewhere/zero/My\\040Documents rw
103 25 0:59 / /home/zero/phone rw,nosuid,nodev,noatime shared:62 - fuse.sshfs everywhere:/storage/emulated/0 rw,user_id=1000
104 25 0:60 / /run/user/1000/doc rw,nosuid,nodev,relatime shared:63 - fuse.portal portal rw
105 25 0:61 / /tmp rw,nosuid,nodev shared:64 - tmpfs tmpfs rw
";

    #[test]
    fn parses_only_remote_mounts_and_unescapes_paths() {
        let mounts = parse_mountinfo(MOUNTINFO);
        let mount_points: Vec<&str> = mounts
            .iter()
            .map(|mount| mount.mount_point.to_str().unwrap())
            .collect();

        assert_eq!(
            mount_points,
            vec![
                "/mnt/somewhere",
                "/mnt/somewhere/My Documents",
                "/home/zero/phone"
            ]
        );
        assert_eq!(mounts[1].source, "//somewhere/zero/My Documents");
        assert_eq!(mounts[2].file_system, "fuse.sshfs");
        assert_eq!(mounts[2].source, "everywhere:/storage/emulated/0");
    }

    #[test]
    fn leaves_lone_backslashes_alone() {
        assert_eq!(unescape_mountinfo("a\\b"), "a\\b");
        assert_eq!(unescape_mountinfo("trailing\\"), "trailing\\");
        assert_eq!(unescape_mountinfo("tab\\011here"), "tab\there");
    }

    #[test]
    fn containing_mount_is_the_innermost_one() {
        let table = MountTable::from_mounts(parse_mountinfo(MOUNTINFO));
        let innermost = table
            .mounts
            .iter()
            .filter(|mount| {
                Path::new("/mnt/somewhere/My Documents/report.txt").starts_with(&mount.mount_point)
            })
            .max_by_key(|mount| mount.mount_point.as_os_str().len())
            .unwrap();
        assert_eq!(
            innermost.mount_point,
            Path::new("/mnt/somewhere/My Documents")
        );
        assert!(table
            .by_mount_point
            .contains_key(Path::new("/home/zero/phone")));
        assert!(!table.by_mount_point.contains_key(Path::new("/home/zero")));
    }

    #[test]
    fn network_filesystem_list_matches_case_insensitively() {
        assert!(is_network_filesystem("CIFS"));
        assert!(is_network_filesystem("fuse.sshfs"));
        assert!(!is_network_filesystem("ext4"));
        assert!(!is_network_filesystem("fuse.portal"));
    }

    #[test]
    fn a_local_path_is_never_gated() {
        assert!(!is_unresponsive_path(Path::new("/")));
        assert!(!is_unresponsive_mount_point(Path::new("/")));
        assert!(ensure_responsive(Path::new("/")).is_ok());
    }
}
