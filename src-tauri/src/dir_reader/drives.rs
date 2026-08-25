// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use crate::utils::normalize_path;
use std::collections::HashSet;

#[cfg(target_os = "macos")]
use super::drives_platform::{
    append_macos_network_volumes, mount_point_last_component, should_skip_macos_mount,
};
#[cfg(windows)]
use super::drives_platform::{append_windows_network_drives, append_windows_wsl_drives};
#[cfg(target_os = "linux")]
use super::drives_platform::{mount_point_last_component, should_skip_linux_mount};
use super::types::DriveInfo;
use sysinfo::Disks;

pub fn get_system_drives() -> Result<Vec<DriveInfo>, String> {
    let disks = Disks::new_with_refreshed_list();
    let mut drives: Vec<DriveInfo> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();

    for disk in disks.iter() {
        let mount_point = disk.mount_point().to_string_lossy().to_string();
        let path = normalize_path(&mount_point);

        let total_space = disk.total_space();
        let available_space = disk.available_space();

        #[cfg(target_os = "linux")]
        if total_space == 0
            || should_skip_linux_mount(
                &disk.file_system().to_string_lossy(),
                &disk.name().to_string_lossy(),
                &mount_point,
            )
        {
            continue;
        }

        #[cfg(target_os = "macos")]
        if total_space == 0 || should_skip_macos_mount(&mount_point) {
            continue;
        }

        #[cfg(windows)]
        if total_space == 0 {
            continue;
        }

        if !seen_paths.insert(path.clone()) {
            continue;
        }

        let used_space = total_space.saturating_sub(available_space);
        let percent_used = if total_space > 0 {
            ((used_space as f64 / total_space as f64) * 100.0).round()
        } else {
            0.0
        };

        let file_system_str = disk.file_system().to_string_lossy().to_lowercase();
        let is_network_fs = matches!(
            file_system_str.as_str(),
            "nfs"
                | "nfs4"
                | "cifs"
                | "smbfs"
                | "fuse.sshfs"
                | "fuse.rclone"
                | "fuse.gvfsd-fuse"
                | "afpfs"
        );

        let drive_type = if is_network_fs {
            "Network".to_string()
        } else {
            match disk.kind() {
                sysinfo::DiskKind::HDD => "HDD".to_string(),
                sysinfo::DiskKind::SSD => "SSD".to_string(),
                sysinfo::DiskKind::Unknown(_) => "Unknown".to_string(),
            }
        };

        let display_name = {
            #[cfg(windows)]
            {
                let volume_label = disk.name().to_string_lossy().to_string();
                if volume_label.is_empty() {
                    format!("Local Disk ({})", mount_point.trim_end_matches('\\'))
                } else {
                    format!("{} ({})", volume_label, mount_point.trim_end_matches('\\'))
                }
            }
            #[cfg(target_os = "linux")]
            {
                if mount_point == "/" {
                    "/".to_string()
                } else {
                    mount_point_last_component(&mount_point)
                }
            }
            #[cfg(target_os = "macos")]
            {
                let volume_label = disk.name().to_string_lossy().to_string();
                if volume_label.is_empty() {
                    mount_point_last_component(&mount_point)
                } else {
                    volume_label
                }
            }
        };

        let device_path = disk.name().to_string_lossy().to_string();

        drives.push(DriveInfo {
            name: display_name,
            path,
            mount_point,
            file_system: disk.file_system().to_string_lossy().to_string(),
            drive_type,
            total_space,
            available_space,
            used_space,
            percent_used,
            is_removable: disk.is_removable(),
            is_read_only: disk.is_read_only(),
            is_mounted: true,
            device_path,
        });
    }

    #[cfg(target_os = "macos")]
    append_macos_network_volumes(&mut drives, &mut seen_paths);

    #[cfg(windows)]
    append_windows_network_drives(&mut drives, &mut seen_paths);

    #[cfg(windows)]
    append_windows_wsl_drives(&mut drives, &mut seen_paths);

    collapse_network_submounts(&mut drives);

    drives.sort_by(|first, second| first.path.cmp(&second.path));

    Ok(drives)
}

/// Drops network mounts that are nested inside another network mount of the
/// same source. The kernel CIFS client automounts NTFS junctions and DFS
/// referrals as separate filesystems the first time they are touched (a shared
/// Windows profile folder yields one per legacy junction: `Application Data`,
/// `Cookies`, `Local Settings`, ...). They are subfolders of the share, not
/// shares of their own, so they must not become separate entries.
fn collapse_network_submounts(drives: &mut Vec<DriveInfo>) {
    let parents: Vec<(String, String)> = drives
        .iter()
        .filter(|drive| drive.drive_type == "Network")
        .map(|drive| (drive.path.clone(), drive.device_path.clone()))
        .collect();

    drives.retain(|drive| {
        if drive.drive_type != "Network" {
            return true;
        }
        !parents.iter().any(|(parent_path, parent_source)| {
            drive.path != *parent_path
                && is_nested_path(&drive.path, parent_path)
                && is_nested_source(&drive.device_path, parent_source)
        })
    });
}

fn is_nested_path(child: &str, parent: &str) -> bool {
    let parent = parent.trim_end_matches(['/', '\\']);
    if parent.is_empty() {
        return false;
    }
    child.len() > parent.len()
        && child.starts_with(parent)
        && matches!(child.as_bytes()[parent.len()], b'/' | b'\\')
}

/// A submount's source is the parent's source plus a subpath
/// (`//host/share/sub` under `//host/share`). Sources that are empty or
/// identical (some FUSE backends report the same label for every mount) are
/// not treated as nested, so unrelated mounts under a common prefix survive.
fn is_nested_source(child: &str, parent: &str) -> bool {
    if parent.is_empty() || child.is_empty() {
        return false;
    }
    is_nested_path(child, parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(path: &str, source: &str, drive_type: &str) -> DriveInfo {
        DriveInfo {
            name: path.to_string(),
            path: path.to_string(),
            mount_point: path.to_string(),
            file_system: "cifs".to_string(),
            drive_type: drive_type.to_string(),
            total_space: 1,
            available_space: 1,
            used_space: 0,
            percent_used: 0.0,
            is_removable: false,
            is_read_only: false,
            is_mounted: true,
            device_path: source.to_string(),
        }
    }

    #[test]
    fn collapses_cifs_junction_submounts_into_the_share() {
        let mut drives = vec![
            drive("/mnt/somewhere", "//host/zero", "Network"),
            drive("/mnt/somewhere/Cookies", "//host/zero/Cookies", "Network"),
            drive(
                "/mnt/somewhere/Documents/My Pictures",
                "//host/zero/Documents/My Pictures",
                "Network",
            ),
        ];
        collapse_network_submounts(&mut drives);
        let paths: Vec<&str> = drives.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, vec!["/mnt/somewhere"]);
    }

    #[test]
    fn keeps_distinct_shares_mounted_under_a_common_directory() {
        let mut drives = vec![
            drive("/mnt/nas", "//nas/media", "Network"),
            drive("/mnt/nas/backup", "//nas/backup", "Network"),
            drive("/mnt/nas2", "//nas/media2", "Network"),
        ];
        collapse_network_submounts(&mut drives);
        assert_eq!(drives.len(), 3);
    }

    #[test]
    fn never_collapses_local_disks() {
        let mut drives = vec![
            drive("/", "/dev/nvme0n1p2", "SSD"),
            drive("/home", "/dev/nvme0n1p2", "SSD"),
            drive("/mnt/share", "//host/share", "Network"),
        ];
        collapse_network_submounts(&mut drives);
        assert_eq!(drives.len(), 3);
    }
}
