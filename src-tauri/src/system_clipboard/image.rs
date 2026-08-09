// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
// Copyright © 2026 Cortexist, LLC (modifications). All rights reserved.

use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use image::codecs::png::PngEncoder;
use image::ImageEncoder;

use crate::utils::unique_path_with_index;

use super::is_valid_png_bytes;
use super::types::{
    SystemClipboardImageInfo, SystemClipboardImagePasteResult, SystemClipboardImagePngPayload,
    SystemClipboardSavedImage,
};
#[cfg(target_os = "windows")]
use super::windows::{
    set_windows_clipboard_bytes, windows_open_clipboard, with_windows_clipboard,
    MAX_CLIPBOARD_PNG_BYTES,
};

struct ClipboardImageBytes {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

/// The targets a PNG is offered and looked for under on Wayland. `image/png` is what modern
/// applications ask for; the others cover the ones that only know an older spelling.
#[cfg(target_os = "linux")]
const WAYLAND_PNG_MIME_TYPES: [&str; 3] = ["image/png", "image/x-png", "PNG"];

pub(crate) fn set_system_clipboard_image_from_png_bytes_sync(
    png_bytes: &[u8],
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        return set_system_clipboard_image_from_png_bytes_inner(png_bytes);
    }

    #[cfg(not(target_os = "windows"))]
    {
        set_system_clipboard_image_from_png_bytes_inner(png_bytes)
    }
}

fn set_system_clipboard_image_from_png_bytes_inner(png_bytes: &[u8]) -> Result<(), String> {
    if png_bytes.is_empty() {
        return Err("Clipboard image payload is empty".to_string());
    }

    #[cfg(target_os = "windows")]
    if png_bytes.len() > MAX_CLIPBOARD_PNG_BYTES {
        return Err(format!(
            "Clipboard image exceeds maximum size of {} bytes",
            MAX_CLIPBOARD_PNG_BYTES
        ));
    }

    if !is_valid_png_bytes(png_bytes) {
        return Err("Clipboard image payload is not a valid PNG".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        return windows_set_clipboard_png_bytes(png_bytes);
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::borrow::Cow;

        // `arboard` writes images through X11 only, and nothing bridges that back to a
        // Wayland compositor's clipboard: the image lands somewhere no Wayland application
        // can paste from. Offering it natively first is the same split file paths already
        // take, and a compositor without `wlr-data-control` falls through unchanged.
        #[cfg(target_os = "linux")]
        {
            if wayland_set_clipboard_png_bytes(png_bytes).is_ok() {
                return Ok(());
            }
        }

        let decoded_image = image::load_from_memory(png_bytes)
            .map_err(|error| format!("Failed to decode PNG clipboard image: {error}"))?;
        let rgba_image = decoded_image.to_rgba8();
        let (width, height) = rgba_image.dimensions();
        let width =
            usize::try_from(width).map_err(|_| "Clipboard image is too wide".to_string())?;
        let height =
            usize::try_from(height).map_err(|_| "Clipboard image is too tall".to_string())?;
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|error| format!("Failed to open system clipboard: {error}"))?;

        clipboard
            .set_image(arboard::ImageData {
                width,
                height,
                bytes: Cow::Owned(rgba_image.into_raw()),
            })
            .map_err(|error| format!("Failed to write image to system clipboard: {error}"))?;

        Ok(())
    }
}

/// Offers the PNG on the Wayland clipboard under the targets image consumers ask for.
#[cfg(target_os = "linux")]
fn wayland_set_clipboard_png_bytes(png_bytes: &[u8]) -> Result<(), String> {
    use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

    let sources = WAYLAND_PNG_MIME_TYPES
        .into_iter()
        .map(|mime_type| MimeSource {
            source: Source::Bytes(png_bytes.to_vec().into_boxed_slice()),
            mime_type: MimeType::Specific(mime_type.to_string()),
        })
        .collect();

    let mut options = Options::new();
    // Serve the offer from a background thread. Blocking here would hold the clipboard
    // command until the next application took ownership.
    options.foreground(false);
    options
        .copy_multi(sources)
        .map_err(|error| error.to_string())
}

pub(crate) fn read_system_clipboard_image_info_sync(
) -> Result<Option<SystemClipboardImageInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        windows_read_clipboard_image_info()
    }

    #[cfg(not(target_os = "windows"))]
    {
        unix_read_clipboard_image_info()
    }
}

/// Reads a PNG off the Wayland clipboard, under any of the targets we also offer.
///
/// `None` means "nothing this layer can answer" — no Wayland, or no image on the clipboard —
/// and every caller falls back to `arboard`.
#[cfg(target_os = "linux")]
fn wayland_read_clipboard_png() -> Option<Vec<u8>> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{get_contents, ClipboardType, MimeType, Seat};

    for mime_type in WAYLAND_PNG_MIME_TYPES {
        let Ok((mut reader, _)) = get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific(mime_type),
        ) else {
            continue;
        };

        let mut buffer = Vec::new();

        if reader.read_to_end(&mut buffer).is_ok() && is_valid_png_bytes(&buffer) {
            return Some(buffer);
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn unix_read_clipboard_image_info() -> Result<Option<SystemClipboardImageInfo>, String> {
    #[cfg(target_os = "linux")]
    if let Some(image) = wayland_read_clipboard_image_bytes()? {
        return Ok(Some(SystemClipboardImageInfo {
            width: image.width as usize,
            height: image.height as usize,
            size_bytes: image.bytes.len(),
            clipboard_sequence: None,
        }));
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;

    match clipboard.get_image() {
        Ok(image) => Ok(Some(SystemClipboardImageInfo {
            width: image.width,
            height: image.height,
            size_bytes: image.bytes.len(),
            clipboard_sequence: None,
        })),
        Err(_) => Ok(None),
    }
}

/// The Wayland clipboard entry as raw pixels, matching what `arboard` hands back.
#[cfg(target_os = "linux")]
fn wayland_read_clipboard_image_bytes() -> Result<Option<ClipboardImageBytes>, String> {
    let Some(png_bytes) = wayland_read_clipboard_png() else {
        return Ok(None);
    };

    let rgba_image = image::load_from_memory(&png_bytes)
        .map_err(|error| format!("Failed to decode PNG clipboard image: {error}"))?
        .to_rgba8();
    let (width, height) = rgba_image.dimensions();

    Ok(Some(ClipboardImageBytes {
        width,
        height,
        bytes: rgba_image.into_raw(),
    }))
}

pub(crate) fn save_system_clipboard_image_to_temp_sync(
) -> Result<Option<SystemClipboardSavedImage>, String> {
    let temp_dir = system_clipboard_image_temp_dir()?;
    cleanup_clipboard_temp_images(&temp_dir)?;
    let destination_file = temp_dir.join("clipboard-image.png");

    #[cfg(target_os = "windows")]
    {
        if let Some(saved_image) = windows_save_system_clipboard_png(&destination_file)? {
            Ok(Some(saved_image))
        } else {
            save_clipboard_image_bytes_to_temp(&destination_file)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        save_clipboard_image_bytes_to_temp(&destination_file)
    }
}

pub(crate) fn read_system_clipboard_image_png_bytes_sync(
) -> Result<Option<SystemClipboardImagePngPayload>, String> {
    let image_info = read_system_clipboard_image_info_sync()?;
    let Some(image_info) = image_info else {
        return Ok(None);
    };

    let saved_image = save_system_clipboard_image_to_temp_sync()?;
    let Some(saved_image) = saved_image else {
        return Ok(None);
    };

    let png_bytes = fs::read(&saved_image.path).map_err(|error| error.to_string())?;

    Ok(Some(SystemClipboardImagePngPayload {
        width: image_info.width,
        height: image_info.height,
        size_bytes: saved_image.size_bytes,
        png_bytes,
    }))
}

pub(crate) fn paste_system_clipboard_image_sync(
    destination_path: &str,
) -> Result<SystemClipboardImagePasteResult, String> {
    let destination = Path::new(destination_path);

    if !destination.exists() {
        return Ok(clipboard_image_paste_result(
            false,
            Some(format!(
                "Destination path does not exist: {}",
                destination_path
            )),
            None,
            Some(1),
            None,
        ));
    }

    if !destination.is_dir() {
        return Ok(clipboard_image_paste_result(
            false,
            Some(format!(
                "Destination is not a directory: {}",
                destination_path
            )),
            None,
            Some(1),
            None,
        ));
    }

    #[cfg(target_os = "windows")]
    {
        Ok(clipboard_image_paste_result(
            false,
            Some("Clipboard image must be saved before paste".to_string()),
            Some(0),
            Some(1),
            None,
        ))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let image = match read_system_clipboard_image_bytes() {
            Ok(Some(image)) => image,
            Ok(None) => {
                return Ok(clipboard_image_paste_result(
                    false,
                    None,
                    Some(0),
                    Some(0),
                    None,
                ));
            }
            Err(error) => {
                return Ok(clipboard_image_paste_result(
                    false,
                    Some(error),
                    None,
                    Some(1),
                    None,
                ));
            }
        };

        let destination_file = unique_path_with_index(
            &destination.join("clipboard-image.png"),
            1,
            "clipboard-image",
            Some("png"),
            None,
        );
        write_clipboard_image_to_png(&destination_file, image)?;

        Ok(clipboard_image_paste_result(
            true,
            None,
            Some(1),
            Some(0),
            Some(destination_file.to_string_lossy().into_owned()),
        ))
    }
}

pub(crate) fn paste_saved_clipboard_image_sync(
    source_path: &str,
    destination_path: &str,
) -> Result<SystemClipboardImagePasteResult, String> {
    let source = Path::new(source_path);
    let destination = Path::new(destination_path);

    if !source.is_file() {
        return Ok(clipboard_image_paste_result(
            false,
            Some(format!(
                "Saved clipboard image does not exist: {source_path}"
            )),
            None,
            Some(1),
            None,
        ));
    }

    if !destination.exists() {
        return Ok(clipboard_image_paste_result(
            false,
            Some(format!(
                "Destination path does not exist: {}",
                destination_path
            )),
            None,
            Some(1),
            None,
        ));
    }

    if !destination.is_dir() {
        return Ok(clipboard_image_paste_result(
            false,
            Some(format!(
                "Destination is not a directory: {}",
                destination_path
            )),
            None,
            Some(1),
            None,
        ));
    }

    let destination_file = unique_path_with_index(
        &destination.join("clipboard-image.png"),
        1,
        "clipboard-image",
        Some("png"),
        None,
    );

    fs::copy(source, &destination_file).map_err(|error| error.to_string())?;

    Ok(clipboard_image_paste_result(
        true,
        None,
        Some(1),
        Some(0),
        Some(destination_file.to_string_lossy().into_owned()),
    ))
}

fn system_clipboard_image_temp_dir() -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir()
        .join("sigma-file-manager")
        .join("clipboard");
    fs::create_dir_all(&temp_dir)
        .map_err(|error| format!("Failed to create clipboard temp directory: {error}"))?;
    Ok(temp_dir)
}

fn cleanup_clipboard_temp_images(temp_dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(temp_dir).map_err(|error| error.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|extension| extension.to_str()) == Some("png") {
            let _ = fs::remove_file(path);
        }
    }

    Ok(())
}

fn save_clipboard_image_bytes_to_temp(
    destination_file: &Path,
) -> Result<Option<SystemClipboardSavedImage>, String> {
    let image = match read_system_clipboard_image_bytes()? {
        Some(image) => image,
        None => return Ok(None),
    };
    write_clipboard_image_to_png(destination_file, image)?;
    let size_bytes = fs::metadata(destination_file)
        .map_err(|error| error.to_string())?
        .len();

    Ok(Some(SystemClipboardSavedImage {
        path: destination_file.to_string_lossy().into_owned(),
        size_bytes,
    }))
}

fn read_system_clipboard_image_bytes() -> Result<Option<ClipboardImageBytes>, String> {
    #[cfg(target_os = "linux")]
    if let Some(image) = wayland_read_clipboard_image_bytes()? {
        return Ok(Some(image));
    }

    let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
    let image = match clipboard.get_image() {
        Ok(image) => image,
        Err(_) => return Ok(None),
    };

    let width =
        u32::try_from(image.width).map_err(|_| "Clipboard image is too wide".to_string())?;
    let height =
        u32::try_from(image.height).map_err(|_| "Clipboard image is too tall".to_string())?;
    let expected_byte_len = image
        .width
        .checked_mul(image.height)
        .and_then(|pixel_count| pixel_count.checked_mul(4))
        .ok_or_else(|| "Clipboard image dimensions are too large".to_string())?;
    let bytes = image.bytes.into_owned();

    if bytes.len() != expected_byte_len {
        return Err("Clipboard image has unexpected pixel data size".to_string());
    }

    Ok(Some(ClipboardImageBytes {
        width,
        height,
        bytes,
    }))
}

fn write_clipboard_image_to_png(
    destination_file: &Path,
    image: ClipboardImageBytes,
) -> Result<(), String> {
    let file = File::create(destination_file).map_err(|error| error.to_string())?;
    let writer = BufWriter::new(file);
    let encoder = PngEncoder::new(writer);
    encoder
        .write_image(
            &image.bytes,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn clipboard_image_paste_result(
    success: bool,
    error: Option<String>,
    copied_count: Option<u32>,
    failed_count: Option<u32>,
    path: Option<String>,
) -> SystemClipboardImagePasteResult {
    SystemClipboardImagePasteResult {
        success,
        error,
        copied_count,
        failed_count,
        skipped_count: Some(0),
        path,
    }
}

#[cfg(target_os = "windows")]
fn png_bytes_to_cf_dib(png_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoded_image = image::load_from_memory(png_bytes)
        .map_err(|error| format!("Failed to decode PNG clipboard image: {error}"))?;
    let rgba_image = decoded_image.to_rgba8();
    let (width, height) = rgba_image.dimensions();

    if width == 0 || height == 0 {
        return Err("Clipboard image dimensions must be greater than zero".to_string());
    }

    let width_usize =
        usize::try_from(width).map_err(|_| "Clipboard image width is too large".to_string())?;
    let height_usize =
        usize::try_from(height).map_err(|_| "Clipboard image height is too large".to_string())?;
    let row_stride = width_usize
        .checked_mul(4)
        .ok_or_else(|| "Clipboard image row stride overflow".to_string())?;
    let pixel_data_size = row_stride
        .checked_mul(height_usize)
        .ok_or_else(|| "Clipboard image pixel buffer overflow".to_string())?;

    const BITMAP_INFO_HEADER_SIZE: usize = 40;
    let mut dib_bytes = vec![0_u8; BITMAP_INFO_HEADER_SIZE + pixel_data_size];

    dib_bytes[0..4].copy_from_slice(&(BITMAP_INFO_HEADER_SIZE as u32).to_le_bytes());
    dib_bytes[4..8].copy_from_slice(&(width as i32).to_le_bytes());
    dib_bytes[8..12].copy_from_slice(&(height as i32).to_le_bytes());
    dib_bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib_bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
    dib_bytes[20..24].copy_from_slice(&(pixel_data_size as u32).to_le_bytes());

    for row_index in 0..height_usize {
        let source_row = height_usize - 1 - row_index;
        let row_start = BITMAP_INFO_HEADER_SIZE + row_index * row_stride;

        for column_index in 0..width_usize {
            let pixel = rgba_image.get_pixel(column_index as u32, source_row as u32);
            let pixel_offset = row_start + column_index * 4;
            dib_bytes[pixel_offset] = pixel[2];
            dib_bytes[pixel_offset + 1] = pixel[1];
            dib_bytes[pixel_offset + 2] = pixel[0];
            dib_bytes[pixel_offset + 3] = pixel[3];
        }
    }

    Ok(dib_bytes)
}

#[cfg(target_os = "windows")]
fn windows_set_clipboard_png_bytes(png_bytes: &[u8]) -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, RegisterClipboardFormatW,
    };

    with_windows_clipboard(|| unsafe {
        windows_open_clipboard()?;
        let clipboard_result = (|| {
            EmptyClipboard().map_err(|error| format!("EmptyClipboard failed: {error}"))?;

            let png_formats = [
                RegisterClipboardFormatW(w!("PNG")),
                RegisterClipboardFormatW(w!("image/png")),
            ];

            for format in png_formats {
                set_windows_clipboard_bytes(format, png_bytes)?;
            }

            const CF_DIB: u32 = 8;
            let dib_bytes = png_bytes_to_cf_dib(png_bytes)?;
            set_windows_clipboard_bytes(CF_DIB, &dib_bytes)?;

            Ok(())
        })();
        let _ = CloseClipboard();
        clipboard_result
    })
}

#[cfg(target_os = "windows")]
fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let value_bytes: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(value_bytes))
}

#[cfg(target_os = "windows")]
fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let value_bytes: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(value_bytes))
}

#[cfg(target_os = "windows")]
fn read_i32_le(bytes: &[u8], offset: usize) -> Option<i32> {
    let value_bytes: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(value_bytes))
}

#[cfg(target_os = "windows")]
fn parse_dib_image_info(
    bytes: &[u8],
    size_bytes: usize,
    clipboard_sequence: Option<u32>,
) -> Option<SystemClipboardImageInfo> {
    let header_size = read_u32_le(bytes, 0)?;

    if header_size == 12 {
        return Some(SystemClipboardImageInfo {
            width: usize::from(read_u16_le(bytes, 4)?),
            height: usize::from(read_u16_le(bytes, 6)?),
            size_bytes,
            clipboard_sequence,
        });
    }

    if header_size < 40 {
        return None;
    }

    let width = read_i32_le(bytes, 4)?;
    let height = read_i32_le(bytes, 8)?;

    if width == 0 || height == 0 {
        return None;
    }

    Some(SystemClipboardImageInfo {
        width: usize::try_from(width.unsigned_abs()).ok()?,
        height: usize::try_from(height.unsigned_abs()).ok()?,
        size_bytes,
        clipboard_sequence,
    })
}

#[cfg(target_os = "windows")]
fn windows_read_clipboard_dib_image_info(
    format: u32,
    clipboard_sequence: Option<u32>,
) -> Result<Option<SystemClipboardImageInfo>, String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    unsafe {
        if IsClipboardFormatAvailable(format).is_err() {
            return Ok(None);
        }

        let clipboard_handle = GetClipboardData(format)
            .map_err(|error| format!("GetClipboardData image format {format} failed: {error}"))?;
        let global_handle = HGLOBAL(clipboard_handle.0);
        let size_bytes = GlobalSize(global_handle);

        if size_bytes == 0 {
            return Ok(None);
        }

        let locked_pointer = GlobalLock(global_handle);

        if locked_pointer.is_null() {
            return Ok(None);
        }

        let bytes = std::slice::from_raw_parts(locked_pointer as *const u8, size_bytes);
        let image_info = parse_dib_image_info(bytes, size_bytes, clipboard_sequence);
        let _ = GlobalUnlock(global_handle);

        Ok(image_info)
    }
}

#[cfg(target_os = "windows")]
fn windows_read_clipboard_image_info() -> Result<Option<SystemClipboardImageInfo>, String> {
    with_windows_clipboard(windows_read_clipboard_image_info_inner)
}

#[cfg(target_os = "windows")]
fn windows_read_clipboard_image_info_inner() -> Result<Option<SystemClipboardImageInfo>, String> {
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardSequenceNumber};

    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;

    unsafe {
        windows_open_clipboard()?;
        let clipboard_sequence = Some(GetClipboardSequenceNumber());
        let clipboard_result =
            match windows_read_clipboard_dib_image_info(CF_DIBV5, clipboard_sequence) {
                Ok(Some(image_info)) => Ok(Some(image_info)),
                Ok(None) | Err(_) => {
                    windows_read_clipboard_dib_image_info(CF_DIB, clipboard_sequence)
                }
            };
        let _ = CloseClipboard();
        clipboard_result
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_read_clipboard_format_bytes(format: u32) -> Result<Option<Vec<u8>>, String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    if format == 0 || IsClipboardFormatAvailable(format).is_err() {
        return Ok(None);
    }

    let clipboard_handle = GetClipboardData(format)
        .map_err(|error| format!("GetClipboardData format {format} failed: {error}"))?;
    let global_handle = HGLOBAL(clipboard_handle.0);
    let size_bytes = GlobalSize(global_handle);

    if size_bytes == 0 {
        return Ok(None);
    }

    let locked_pointer = GlobalLock(global_handle);

    if locked_pointer.is_null() {
        return Ok(None);
    }

    let bytes = std::slice::from_raw_parts(locked_pointer as *const u8, size_bytes).to_vec();
    let _ = GlobalUnlock(global_handle);

    Ok(Some(bytes))
}

#[cfg(target_os = "windows")]
fn windows_save_system_clipboard_png(
    destination_file: &Path,
) -> Result<Option<SystemClipboardSavedImage>, String> {
    with_windows_clipboard(|| windows_save_system_clipboard_png_inner(destination_file))
}

#[cfg(target_os = "windows")]
fn windows_save_system_clipboard_png_inner(
    destination_file: &Path,
) -> Result<Option<SystemClipboardSavedImage>, String> {
    use windows::core::w;
    use windows::Win32::System::DataExchange::{CloseClipboard, RegisterClipboardFormatW};

    unsafe {
        windows_open_clipboard()?;
        let clipboard_result = (|| {
            let png_formats = [
                RegisterClipboardFormatW(w!("PNG")),
                RegisterClipboardFormatW(w!("image/png")),
            ];

            for format in png_formats {
                if let Some(bytes) = windows_read_clipboard_format_bytes(format)? {
                    if bytes.len() > MAX_CLIPBOARD_PNG_BYTES || !is_valid_png_bytes(&bytes) {
                        continue;
                    }

                    fs::write(destination_file, &bytes).map_err(|error| error.to_string())?;

                    return Ok(Some(SystemClipboardSavedImage {
                        path: destination_file.to_string_lossy().into_owned(),
                        size_bytes: bytes.len() as u64,
                    }));
                }
            }

            Ok(None)
        })();
        let _ = CloseClipboard();
        clipboard_result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(windows)]
    fn png_bytes_to_cf_dib_builds_bitmap_header_and_pixels() {
        use super::png_bytes_to_cf_dib;
        use image::ImageEncoder;

        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        encoder
            .write_image(
                &[255, 0, 0, 255, 0, 255, 0, 255],
                1,
                2,
                image::ExtendedColorType::Rgba8,
            )
            .expect("png encode");

        let dib_bytes = png_bytes_to_cf_dib(&png_bytes).expect("dib conversion");
        assert!(dib_bytes.len() > 40);
        assert_eq!(
            u32::from_le_bytes(dib_bytes[0..4].try_into().expect("header size")),
            40
        );
        assert_eq!(
            u32::from_le_bytes(dib_bytes[4..8].try_into().expect("width")),
            1
        );
        assert_eq!(
            u32::from_le_bytes(dib_bytes[8..12].try_into().expect("height")),
            2
        );
    }

    /// Manual check: decodes a real video frame, puts it on the clipboard and holds it there
    /// long enough to paste somewhere else or read back with `wl-paste --type image/png`.
    /// Ignored because it needs a desktop session and GStreamer plugins. Run with:
    /// `SFM_TEST_VIDEO=/path/to.mp4 cargo test --lib -- --ignored holds_a_video_frame`
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn holds_a_video_frame_on_the_clipboard() {
        let path = std::env::var("SFM_TEST_VIDEO").expect("SFM_TEST_VIDEO must be set");
        let png =
            crate::video_thumbnails::capture_video_frame_png(&path, 2.0).expect("frame decodes");

        super::set_system_clipboard_image_from_png_bytes_sync(&png).expect("clipboard write");

        println!("holding {} PNG bytes on the clipboard", png.len());
        std::thread::sleep(std::time::Duration::from_secs(20));
    }
}
