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
//! The registry probes each remote mount with a `statvfs` in a helper process (a thread
//! would do the same work but could not be shed on exit; see [`PROBE_MOUNT_CLI_FLAG`]). A
//! probe that has not answered within [`PROBE_DEADLINE`] marks the mount unresponsive; the
//! helper is left to finish on its own (it will, once the transport times out) and its late
//! answer flips the mount back. At most one probe per mount is ever in flight. Mounts the
//! kernel automounted inside another share (CIFS junctions and DFS referrals) share their
//! parent's probe: they are one transport. Every state change is broadcast to the webviews
//! as a [`MOUNT_HEALTH_CHANGED_EVENT`].
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
/// Each probe is a short-lived helper process (see [`PROBE_MOUNT_CLI_FLAG`]), so rechecks are
/// spaced to keep that cheap even with a dozen shares mounted.
const RESPONSIVE_RECHECK: Duration = Duration::from_secs(15);
const UNRESPONSIVE_RECHECK: Duration = Duration::from_secs(15);
const OVERDUE_POLL_STEP: Duration = Duration::from_millis(200);
/// Launches this executable as a mount probe: `sigma-file-manager --probe-mount <path>` runs
/// one `statvfs` and exits. A probe must live in its own process. A thread parked in a
/// request the FUSE daemon has already taken is uninterruptible, and a process cannot finish
/// exiting while it owns such a thread: the picker took 30 s to close and the main window
/// hung on quit for as long as a dead sshfs held a probe. A child process parked the same way
/// holds nothing of its parent's.
pub const PROBE_MOUNT_CLI_FLAG: &str = "--probe-mount";
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
    /// The mount whose probe answers for this one: itself, or the share it was automounted
    /// inside of.
    pub probe_target: PathBuf,
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
    fn from_mounts(mut mounts: Vec<NetworkMount>) -> Self {
        // Parents before children, so a child can take its parent's already-resolved target.
        mounts.sort_by_key(|mount| mount.mount_point.as_os_str().len());

        for index in 0..mounts.len() {
            let (earlier, rest) = mounts.split_at_mut(index);
            let mount = &mut rest[0];
            let inherited = earlier
                .iter()
                .rev()
                .find(|parent| {
                    mount.mount_point != parent.mount_point
                        && mount.mount_point.starts_with(&parent.mount_point)
                        && is_nested_source(&mount.source, &parent.source)
                })
                .map(|parent| parent.probe_target.clone());
            mount.probe_target = inherited.unwrap_or_else(|| mount.mount_point.clone());
        }

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

/// A submount's source is its parent's source plus a subpath (`//host/share/sub` under
/// `//host/share`); that is what marks it as a piece of the same share rather than another
/// share mounted under a common directory.
fn is_nested_source(child: &str, parent: &str) -> bool {
    let parent = parent.trim_end_matches(['/', '\\']);
    !parent.is_empty()
        && child.len() > parent.len()
        && child.starts_with(parent)
        && matches!(child.as_bytes()[parent.len()], b'/' | b'\\')
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

            let mount_point = PathBuf::from(unescape_mountinfo(mount_point));
            Some(NetworkMount {
                probe_target: mount_point.clone(),
                mount_point,
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

/// For the mount point of a remote filesystem, what the kernel holds for it — never asking
/// the server. `None` when `path` is not such a mount point.
pub fn mount_point_attributes(path: &Path) -> Option<CachedAttributes> {
    network_mount_at(path)?;
    Some(cached_attributes(path).unwrap_or(CachedAttributes {
        is_dir: true,
        ..Default::default()
    }))
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

    let Some(record) = records.get(&mount.probe_target) else {
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
        .get(&mount.probe_target)
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
        let record = records.entry(mount.probe_target.clone()).or_default();

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

    spawn_probe(mount.probe_target.clone());
}

/// Recognizes a probe launch; the caller runs [`run_probe_process`] and exits.
pub fn probe_mount_arg(args: &[String]) -> Option<PathBuf> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == PROBE_MOUNT_CLI_FLAG {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

/// The whole of a probe process: one `statvfs`, its figures on stdout, exit status as the
/// verdict.
pub fn run_probe_process(mount_point: &Path) -> i32 {
    match statvfs_storage(mount_point) {
        Some(storage) => {
            println!(
                "{} {} {}",
                storage.total_space,
                storage.available_space,
                u8::from(storage.is_read_only)
            );
            0
        }
        None => 1,
    }
}

fn parse_probe_output(output: &str) -> Option<MountStorage> {
    let mut fields = output.split_whitespace();
    let total_space = fields.next()?.parse().ok()?;
    let available_space = fields.next()?.parse().ok()?;
    let is_read_only = fields.next()? == "1";
    Some(MountStorage {
        total_space,
        available_space,
        is_read_only,
    })
}

fn spawn_probe_process(mount_point: &Path) -> std::io::Result<std::process::Child> {
    std::process::Command::new(std::env::current_exe()?)
        .arg(PROBE_MOUNT_CLI_FLAG)
        .arg(mount_point)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
}

fn spawn_probe(mount_point: PathBuf) {
    let probed_mount_point = mount_point.clone();
    let spawned = std::thread::Builder::new()
        .name("mount-probe".to_string())
        .spawn(move || {
            let mount_point = probed_mount_point;
            match spawn_probe_process(&mount_point) {
                Ok(child) => watch_probe_process(&mount_point, child),
                // No helper could be started; the thread does the work itself and the
                // process keeps the exit-time cost that entails.
                Err(_) => probe_in_thread(&mount_point),
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

fn watch_probe_process(mount_point: &Path, mut child: std::process::Child) {
    let started = Instant::now();
    let mut reported_overdue = false;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let storage = if status.success() {
                    let mut output = String::new();
                    if let Some(mut stdout) = child.stdout.take() {
                        use std::io::Read;
                        let _ = stdout.read_to_string(&mut output);
                    }
                    parse_probe_output(&output)
                } else {
                    None
                };
                settle(mount_point, storage);
                return;
            }
            Ok(None) => {}
            Err(_) => {
                settle(mount_point, None);
                return;
            }
        }

        if !reported_overdue && started.elapsed() >= PROBE_DEADLINE {
            report(mount_point, MountHealth::Unresponsive);
            reported_overdue = true;
        }

        std::thread::sleep(if reported_overdue {
            OVERDUE_POLL_STEP
        } else {
            HEALTH_POLL_STEP
        });
    }
}

fn probe_in_thread(mount_point: &Path) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let probe_path = mount_point.to_path_buf();
    let worker = std::thread::Builder::new()
        .name("mount-probe-statvfs".to_string())
        .spawn(move || {
            let _ = sender.send(statvfs_storage(&probe_path));
        });

    if worker.is_err() {
        settle(mount_point, None);
        return;
    }

    match receiver.recv_timeout(PROBE_DEADLINE) {
        Ok(result) => settle(mount_point, result),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            report(mount_point, MountHealth::Unresponsive);
            let late = receiver.recv().ok().flatten();
            settle(mount_point, late);
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => settle(mount_point, None),
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

fn report(probe_target: &Path, health: MountHealth) {
    let changed = {
        let mut records = records()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = records.entry(probe_target.to_path_buf()).or_default();
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

    let Some(app_handle) = APP_HANDLE.get() else {
        return;
    };

    // One event per mount the probe speaks for, so a listing showing a junction submount
    // hears about its share going down.
    let table = mount_table();
    for mount in table
        .mounts
        .iter()
        .filter(|mount| mount.probe_target == probe_target)
    {
        let payload = MountHealthChangedPayload {
            mount_point: crate::utils::normalize_path(&mount.mount_point.to_string_lossy()),
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
    fn junction_submounts_share_their_share_probe() {
        let table = MountTable::from_mounts(parse_mountinfo(MOUNTINFO));
        let target = |mount_point: &str| {
            table.mounts[table.by_mount_point[Path::new(mount_point)]]
                .probe_target
                .clone()
        };
        assert_eq!(
            target("/mnt/somewhere/My Documents"),
            Path::new("/mnt/somewhere")
        );
        assert_eq!(target("/mnt/somewhere"), Path::new("/mnt/somewhere"));
        assert_eq!(target("/home/zero/phone"), Path::new("/home/zero/phone"));
    }

    #[test]
    fn distinct_shares_under_one_directory_keep_their_own_probe() {
        let mounts = vec![
            NetworkMount {
                mount_point: PathBuf::from("/mnt/nas"),
                file_system: "cifs".into(),
                source: "//nas/media".into(),
                probe_target: PathBuf::from("/mnt/nas"),
            },
            NetworkMount {
                mount_point: PathBuf::from("/mnt/nas/backup"),
                file_system: "cifs".into(),
                source: "//nas/backup".into(),
                probe_target: PathBuf::from("/mnt/nas/backup"),
            },
        ];
        let table = MountTable::from_mounts(mounts);
        assert!(table
            .mounts
            .iter()
            .all(|mount| mount.probe_target == mount.mount_point));
    }

    #[test]
    fn probe_launch_is_recognized_and_answers_in_a_fixed_shape() {
        let args: Vec<String> = ["sigma", "--probe-mount", "/"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(probe_mount_arg(&args), Some(PathBuf::from("/")));
        assert_eq!(probe_mount_arg(&args[..1]), None);
        assert_eq!(
            parse_probe_output("1000 250 1\n"),
            Some(MountStorage {
                total_space: 1000,
                available_space: 250,
                is_read_only: true
            })
        );
        assert_eq!(parse_probe_output("garbage"), None);
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
