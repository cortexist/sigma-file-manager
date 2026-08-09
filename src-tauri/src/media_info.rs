// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! What a media file is made of: codecs, resolution, rates, density.
//!
//! A file manager that shows a video's size and duration but not its codec is
//! withholding the field people actually need — whether a file will play on the
//! device they are about to copy it to, why an editor refused it, what a clip
//! was recorded as.
//!
//! Audio and video are read with GStreamer's discoverer, which is the same stack
//! that renders the thumbnail beside it, so no new runtime dependency and no
//! second opinion about what the file contains. GStreamer is a Linux-only
//! dependency here, so that half of the reading is too.
//!
//! Stills are read separately, with the `image` crate. The discoverer does see
//! them, but only as a one-frame video stream — it has no notion of the physical
//! density a scan or an export carries, which is the field that matters for an
//! image. Reading them apart also means image details work on every platform.

use std::path::Path;

#[cfg(target_os = "linux")]
use gstreamer as gst;
#[cfg(target_os = "linux")]
use gstreamer::prelude::*;
#[cfg(target_os = "linux")]
use gstreamer_pbutils::prelude::*;
#[cfg(target_os = "linux")]
use gstreamer_pbutils::Discoverer;

/// A malformed or enormous file must not hold the panel open indefinitely.
#[cfg(target_os = "linux")]
const TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(5);

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStream {
    /// Human wording from GStreamer itself: "H.264 (High Profile)".
    pub codec: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Frames per second, already divided out; `None` for a variable rate.
    pub frame_rate: Option<f64>,
    pub bitrate_bps: Option<u32>,
    /// The decoder this stream would be given, named as GStreamer names it.
    pub decoder: Option<String>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    pub codec: Option<String>,
    pub channels: u32,
    pub sample_rate_hz: u32,
    pub bitrate_bps: Option<u32>,
    /// The decoder this stream would be given, named as GStreamer names it.
    pub decoder: Option<String>,
}

/// What a still is made of.
///
/// Kept apart from the stream lists rather than folded in as a one-frame video: an image has
/// no duration and no rate, and it has a physical density that no video ever states.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    /// "PNG", "JPEG" — for a still the container and the encoding are the same thing.
    pub format: Option<String>,
    pub width: u32,
    pub height: u32,
    /// What one pixel holds, e.g. "RGBA 8-bit".
    pub color: Option<String>,
    /// Dots per inch, and only when the file states one. Most screenshots do not, and a
    /// default of 72 or 96 would be an invention rather than a reading.
    pub dpi: Option<f64>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// The container, when it is worth naming separately from its streams.
    pub container: Option<String>,
    pub duration_ms: Option<u64>,
    pub video: Vec<VideoStream>,
    pub audio: Vec<AudioStream>,
    /// Set instead of the stream lists when the file is a still.
    pub image: Option<ImageInfo>,
}

/// Names the encoding the way a person would: "PNG", not "image/png".
fn describe_format(format: image::ImageFormat) -> String {
    format
        .to_mime_type()
        .strip_prefix("image/")
        .unwrap_or("")
        .to_ascii_uppercase()
}

/// Summarises what one pixel holds. `ColorType` is `non_exhaustive`, hence the fallback.
fn describe_color(color: image::ColorType) -> Option<String> {
    let (channels, bits) = match color {
        image::ColorType::L8 => ("Grayscale", 8),
        image::ColorType::La8 => ("Grayscale + alpha", 8),
        image::ColorType::Rgb8 => ("RGB", 8),
        image::ColorType::Rgba8 => ("RGBA", 8),
        image::ColorType::L16 => ("Grayscale", 16),
        image::ColorType::La16 => ("Grayscale + alpha", 16),
        image::ColorType::Rgb16 => ("RGB", 16),
        image::ColorType::Rgba16 => ("RGBA", 16),
        image::ColorType::Rgb32F => ("RGB", 32),
        image::ColorType::Rgba32F => ("RGBA", 32),
        _ => return None,
    };

    Some(format!("{channels} {bits}-bit"))
}

/// PNG states density in pixels per metre, and only when the unit says so — `Unit::Unspecified`
/// means the numbers are an aspect ratio rather than a physical size.
fn png_dpi(path: &Path) -> Option<f64> {
    const METRES_PER_INCH: f64 = 0.0254;

    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let reader = png::Decoder::new(file).read_info().ok()?;
    let dimensions = reader.info().pixel_dims?;

    (dimensions.unit == png::Unit::Meter && dimensions.xppu > 0)
        .then(|| f64::from(dimensions.xppu) * METRES_PER_INCH)
}

/// Reads density out of a JFIF APP0 segment, which the spec puts immediately after the SOI
/// marker, so the offsets are fixed and no scan is needed.
///
/// Cameras often state density only in EXIF instead, which this deliberately does not parse:
/// an unread density reports as absent, which is honest, where a guessed one would not be.
fn jpeg_dpi(header: &[u8]) -> Option<f64> {
    // SOI, then an APP0 whose payload starts with the "JFIF\0" identifier.
    if header.len() < 18 || header[0..2] != [0xFF, 0xD8] || header[2..4] != [0xFF, 0xE0] {
        return None;
    }

    if &header[6..11] != b"JFIF\0" {
        return None;
    }

    let density = f64::from(u16::from_be_bytes([header[14], header[15]]));

    if density <= 0.0 {
        return None;
    }

    match header[13] {
        1 => Some(density),
        // Dots per centimetre.
        2 => Some(density * 2.54),
        // 0 means the numbers are only an aspect ratio, carrying no physical size.
        _ => None,
    }
}

fn image_dpi(path: &Path, format: image::ImageFormat) -> Option<f64> {
    match format {
        image::ImageFormat::Png => png_dpi(path),
        image::ImageFormat::Jpeg => {
            let mut header = [0u8; 18];
            let mut file = std::fs::File::open(path).ok()?;
            std::io::Read::read_exact(&mut file, &mut header).ok()?;
            jpeg_dpi(&header)
        }
        _ => None,
    }
}

/// `Ok(None)` means "not a still", which is the signal to go and ask GStreamer instead.
fn read_image(path: &str) -> Result<Option<ImageInfo>, String> {
    let path = Path::new(path);

    let Ok(reader) = image::ImageReader::open(path).and_then(|reader| reader.with_guessed_format())
    else {
        return Ok(None);
    };

    let Some(format) = reader.format() else {
        return Ok(None);
    };

    let decoder = reader
        .into_decoder()
        .map_err(|error| format!("Failed to read image details: {error}"))?;

    let (width, height) = image::ImageDecoder::dimensions(&decoder);

    Ok(Some(ImageInfo {
        format: Some(describe_format(format)),
        width,
        height,
        color: describe_color(image::ImageDecoder::color_type(&decoder)),
        dpi: image_dpi(path, format),
    }))
}

/// `24000/1001` is 23.976, and a rate of `0/1` means the file does not say.
#[cfg(target_os = "linux")]
fn frame_rate(fraction: gst::Fraction) -> Option<f64> {
    let (numerator, denominator) = (fraction.numer(), fraction.denom());
    (numerator > 0 && denominator > 0).then(|| f64::from(numerator) / f64::from(denominator))
}

/// Zero is how GStreamer reports "unknown" for a bitrate, which is not the same
/// as a file that genuinely streams at no bits per second.
#[cfg(target_os = "linux")]
fn bitrate(value: u32) -> Option<u32> {
    (value > 0).then_some(value)
}

/// Names the decoder this stream would be handed, e.g. "VA-API H.264 Decoder in AMD Radeon 780M
/// Graphics" or "libav H.264 ... decoder" — which is what says whether a file lands on a GPU or
/// on the CPU, and on *which* GPU when the machine has more than one.
///
/// This is the decoder the registry would select, not one observed decoding: the player runs in
/// a WebKit process that exposes nothing about its pipeline. It is a firm prediction rather than
/// a guess, because that process reads the same registry, the same ranks and the same
/// `GST_PLUGIN_FEATURE_RANK` as this one, and picking the highest-ranked factory whose sink pad
/// accepts the caps is exactly how `decodebin` autoplugs. WebKit can still fall back to software
/// for a profile or size the hardware refuses, and it decides that privately.
#[cfg(target_os = "linux")]
fn describe_decoder(caps: Option<&gst::Caps>) -> Option<String> {
    let caps = caps?;

    // `factories_with_type` hands them back in rank order, so the first that accepts these caps
    // is the one that would win the autoplug.
    let factories = gst::ElementFactory::factories_with_type(
        gst::ElementFactoryType::DECODER,
        gst::Rank::MARGINAL,
    );

    factories
        .iter()
        .find(|factory| factory.can_sink_any_caps(caps))
        .map(|factory| factory.longname().to_string())
}

#[cfg(target_os = "linux")]
fn describe_codec(caps: Option<gst::Caps>) -> Option<String> {
    let caps = caps?;
    let description = gstreamer_pbutils::functions::pb_utils_get_codec_description(&caps);
    let description = description.to_string();

    (!description.is_empty()).then_some(description)
}

/// Discovery is synchronous and may take the whole timeout to give up, so it runs off-thread.
#[cfg(target_os = "linux")]
async fn read(path: String) -> Result<MediaInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        gst::init().map_err(|error| format!("Failed to initialize GStreamer: {error}"))?;

        let uri = gst::glib::filename_to_uri(&path, None)
            .map_err(|error| format!("Failed to build a URI for the file: {error}"))?;

        let discoverer = Discoverer::new(TIMEOUT)
            .map_err(|error| format!("Failed to inspect media: {error}"))?;
        let info = discoverer
            .discover_uri(&uri)
            .map_err(|error| format!("Failed to read media details: {error}"))?;

        let video = info
            .video_streams()
            .iter()
            .filter_map(|stream| {
                let stream = stream.downcast_ref::<gstreamer_pbutils::DiscovererVideoInfo>()?;

                // Cover art rides along as a video stream of a single frame;
                // reporting it as "the video" of an mp3 would be wrong.
                if stream.is_image() {
                    return None;
                }

                Some(VideoStream {
                    codec: describe_codec(stream.caps()),
                    width: stream.width(),
                    height: stream.height(),
                    frame_rate: frame_rate(stream.framerate()),
                    bitrate_bps: bitrate(stream.bitrate()),
                    decoder: describe_decoder(stream.caps().as_ref()),
                })
            })
            .collect();

        let audio = info
            .audio_streams()
            .iter()
            .filter_map(|stream| {
                let stream = stream.downcast_ref::<gstreamer_pbutils::DiscovererAudioInfo>()?;

                Some(AudioStream {
                    codec: describe_codec(stream.caps()),
                    channels: stream.channels(),
                    sample_rate_hz: stream.sample_rate(),
                    bitrate_bps: bitrate(stream.bitrate()),
                    decoder: describe_decoder(stream.caps().as_ref()),
                })
            })
            .collect();

        Ok(MediaInfo {
            container: info
                .stream_info()
                .and_then(|stream| stream.caps())
                .and_then(|caps| describe_codec(Some(caps))),
            duration_ms: info.duration().map(|duration| duration.mseconds()),
            video,
            audio,
            image: None,
        })
    })
    .await
    .map_err(|error| format!("Media inspection task failed: {error}"))?
}

#[tauri::command]
pub async fn media_info(path: String) -> Result<MediaInfo, String> {
    let image_path = path.clone();
    let image = tauri::async_runtime::spawn_blocking(move || read_image(&image_path))
        .await
        .map_err(|error| format!("Image inspection task failed: {error}"))??;

    // A still is the whole answer; there is no second stack worth asking.
    if let Some(image) = image {
        return Ok(MediaInfo {
            image: Some(image),
            ..MediaInfo::default()
        });
    }

    #[cfg(target_os = "linux")]
    {
        read(path).await
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("Audio and video details are only read natively on Linux".to_string())
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;

    /// A JFIF header stating `density` in `units`, padded to the length the parser needs.
    fn jfif_header(units: u8, density: u16) -> [u8; 18] {
        let mut header = [0u8; 18];
        header[0..2].copy_from_slice(&[0xFF, 0xD8]);
        header[2..4].copy_from_slice(&[0xFF, 0xE0]);
        header[6..11].copy_from_slice(b"JFIF\0");
        header[13] = units;
        header[14..16].copy_from_slice(&density.to_be_bytes());
        header
    }

    #[test]
    fn reads_a_stated_density_in_dots_per_inch() {
        assert_eq!(jpeg_dpi(&jfif_header(1, 300)), Some(300.0));
    }

    #[test]
    fn converts_a_density_given_per_centimetre() {
        let dpi = jpeg_dpi(&jfif_header(2, 118)).expect("a density");
        assert!((dpi - 299.72).abs() < 0.01, "{dpi} should be about 300");
    }

    /// Units of 0 means the pair is an aspect ratio, so there is no density to report.
    #[test]
    fn an_aspect_ratio_is_not_a_density() {
        assert_eq!(jpeg_dpi(&jfif_header(0, 72)), None);
        assert_eq!(jpeg_dpi(&jfif_header(1, 0)), None);
    }

    #[test]
    fn a_file_that_is_not_jfif_states_nothing() {
        assert_eq!(jpeg_dpi(&[0u8; 18]), None);
        // Truncated headers must not panic on the fixed offsets.
        assert_eq!(jpeg_dpi(&[0xFF, 0xD8, 0xFF, 0xE0]), None);
    }

    #[test]
    fn names_the_encoding_the_way_a_person_would() {
        assert_eq!(describe_format(image::ImageFormat::Png), "PNG");
        assert_eq!(describe_format(image::ImageFormat::Jpeg), "JPEG");
    }

    #[test]
    fn summarises_what_a_pixel_holds() {
        assert_eq!(
            describe_color(image::ColorType::Rgba8).as_deref(),
            Some("RGBA 8-bit")
        );
        assert_eq!(
            describe_color(image::ColorType::L16).as_deref(),
            Some("Grayscale 16-bit")
        );
    }

    /// Anything that is not a still has to fall through to the audio/video stack.
    #[test]
    fn a_missing_or_non_image_file_is_not_a_still() {
        assert!(matches!(read_image("/nonexistent/nothing.png"), Ok(None)));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_declared_frame_rate_is_divided_out() {
        // NTSC rates are the reason this is not an integer.
        let rate = frame_rate(gst::Fraction::new(24000, 1001)).expect("a rate");
        assert!((rate - 23.976).abs() < 0.001);
        assert_eq!(frame_rate(gst::Fraction::new(30, 1)), Some(30.0));
    }

    /// A variable-rate file reports `0/1`, which must read as "not stated"
    /// rather than as a video running at zero frames per second.
    ///
    /// The zero-denominator case the code also guards cannot be written here:
    /// `Fraction::new` refuses to build one.
    #[test]
    fn an_undeclared_frame_rate_is_absent_not_zero() {
        assert_eq!(frame_rate(gst::Fraction::new(0, 1)), None);
    }

    #[test]
    fn an_unknown_bitrate_is_absent_not_zero() {
        assert_eq!(bitrate(0), None);
        assert_eq!(bitrate(128_000), Some(128_000));
    }

    /// Manual check against a real file, since discovery needs GStreamer plugins
    /// CI may not install. Run with:
    /// `SFM_TEST_MEDIA=/path/to.mp4 cargo test --lib -- --ignored reads_a_real_media_file`
    #[test]
    #[ignore]
    fn reads_a_real_media_file() {
        let path = std::env::var("SFM_TEST_MEDIA").expect("SFM_TEST_MEDIA must be set");
        let info = tauri::async_runtime::block_on(media_info(path)).expect("media reads");

        println!(
            "{}",
            serde_json::to_string_pretty(&info).expect("serializes")
        );

        match &info.image {
            Some(image) => assert!(image.width > 0, "a real still has a width"),
            None => assert!(
                info.duration_ms.is_some(),
                "a real recording has a duration"
            ),
        }
    }
}
