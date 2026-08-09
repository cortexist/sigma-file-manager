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

/// What the camera recorded about a shot.
///
/// Values are taken as EXIF's own display forms — "1/250 s", "f/2.8", "50 mm" — rather than
/// reassembled from rationals here, because the reader already knows each tag's unit and
/// conventional rendering. Coordinates are the exception: they come back as numbers so the
/// front end can render them, and so anything later that wants to open a map has them.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExifInfo {
    /// Make and model folded together, e.g. "Canon EOS R5".
    pub camera: Option<String>,
    pub lens: Option<String>,
    /// When the shutter fired, which is not when the file was written.
    pub taken_at: Option<String>,
    pub exposure_time: Option<String>,
    pub f_number: Option<String>,
    pub iso: Option<String>,
    pub focal_length: Option<String>,
    /// Signed decimal degrees, south and west being negative. Present only when the file still
    /// carries the tags — sharing platforms routinely strip them.
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// The software that last wrote the file, e.g. "Adobe Lightroom".
    pub software: Option<String>,
}

impl ExifInfo {
    fn is_empty(&self) -> bool {
        self.camera.is_none()
            && self.lens.is_none()
            && self.taken_at.is_none()
            && self.exposure_time.is_none()
            && self.f_number.is_none()
            && self.iso.is_none()
            && self.focal_length.is_none()
            && self.latitude.is_none()
            && self.longitude.is_none()
            && self.software.is_none()
    }
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
    /// Absent for anything that was not taken with a camera, which is most files.
    pub exif: Option<ExifInfo>,
}

/// What the file says about itself: ID3 frames, Vorbis comments, MP4 atoms — whatever the
/// container carries, normalised to the handful of fields worth listing.
///
/// These are properties of the file in the way a codec is and a decoder is not, which is why
/// they belong in a properties panel.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub composer: Option<String>,
    pub genre: Option<String>,
    pub track_number: Option<u32>,
    /// The year alone. A release date's day and month are noise in a properties list.
    pub year: Option<i32>,
    /// The software that wrote the file, e.g. "Lavf60.16.100".
    pub encoder: Option<String>,
}

impl MediaTags {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.album_artist.is_none()
            && self.composer.is_none()
            && self.genre.is_none()
            && self.track_number.is_none()
            && self.year.is_none()
            && self.encoder.is_none()
    }
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
    /// Absent when the file carries no tags worth listing, rather than an object of nulls.
    pub tags: Option<MediaTags>,
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

/// EXIF renders ASCII fields with surrounding quotes, and trailing NULs and padding spaces are
/// common in the wild.
fn exif_text(value: String) -> Option<String> {
    let trimmed = value.trim().trim_matches('"').trim_end_matches('\0').trim();

    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Make and model, folded together the way a person would say it. Bodies commonly repeat the
/// make inside the model ("NIKON CORPORATION" + "NIKON D850"), which would read badly joined.
fn camera_name(make: Option<String>, model: Option<String>) -> Option<String> {
    match (make, model) {
        (Some(make), Some(model)) => {
            let first_word = make.split_whitespace().next().unwrap_or(&make);

            if model.to_lowercase().starts_with(&first_word.to_lowercase()) {
                Some(model)
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (make, model) => model.or(make),
    }
}

/// EXIF writes a coordinate as three rationals — degrees, minutes, seconds — with the
/// hemisphere kept in a separate tag, so this half is always positive.
fn dms_to_degrees(parts: &[exif::Rational]) -> Option<f64> {
    let [degrees, minutes, seconds] = parts.get(..3)? else {
        return None;
    };

    Some(degrees.to_f64() + minutes.to_f64() / 60.0 + seconds.to_f64() / 3600.0)
}

/// `S` and `W` are the negative halves of the globe. An absent or unreadable reference leaves
/// the coordinate unsigned rather than guessing a hemisphere.
fn apply_hemisphere(degrees: f64, reference: Option<&str>, negative: char) -> f64 {
    match reference {
        Some(reference) if reference.trim().eq_ignore_ascii_case(&negative.to_string()) => -degrees,
        _ => degrees,
    }
}

fn gps_coordinate(
    exif: &exif::Exif,
    coordinate: exif::Tag,
    hemisphere: exif::Tag,
    negative: char,
) -> Option<f64> {
    let field = exif.get_field(coordinate, exif::In::PRIMARY)?;

    let exif::Value::Rational(parts) = &field.value else {
        return None;
    };

    let degrees = dms_to_degrees(parts)?;
    let reference = exif
        .get_field(hemisphere, exif::In::PRIMARY)
        .and_then(|found| exif_text(found.display_value().to_string()));

    Some(apply_hemisphere(degrees, reference.as_deref(), negative))
}

fn read_exif(path: &Path) -> Option<ExifInfo> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;

    let field = |tag: exif::Tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|found| exif_text(found.display_value().with_unit(&exif).to_string()))
    };

    let read = ExifInfo {
        camera: camera_name(field(exif::Tag::Make), field(exif::Tag::Model)),
        lens: field(exif::Tag::LensModel),
        // When the shutter fired. `DateTime` is when the file was written, which the panel
        // already states from the filesystem.
        taken_at: field(exif::Tag::DateTimeOriginal).or_else(|| field(exif::Tag::DateTime)),
        exposure_time: field(exif::Tag::ExposureTime),
        f_number: field(exif::Tag::FNumber),
        iso: field(exif::Tag::PhotographicSensitivity),
        focal_length: field(exif::Tag::FocalLength),
        latitude: gps_coordinate(
            &exif,
            exif::Tag::GPSLatitude,
            exif::Tag::GPSLatitudeRef,
            'S',
        ),
        longitude: gps_coordinate(
            &exif,
            exif::Tag::GPSLongitude,
            exif::Tag::GPSLongitudeRef,
            'W',
        ),
        software: field(exif::Tag::Software),
    };

    (!read.is_empty()).then_some(read)
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
        exif: read_exif(path),
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

/// A tag present but blank says no more than an absent one, and would draw an empty row.
#[cfg(target_os = "linux")]
fn tag_text(value: &str) -> Option<String> {
    let trimmed = value.trim();

    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(target_os = "linux")]
fn read_tags(tags: &gst::TagList) -> Option<MediaTags> {
    let year = tags
        .get::<gst::tags::DateTime>()
        .map(|value| value.get().year())
        .or_else(|| {
            tags.get::<gst::tags::Date>()
                .map(|value| i32::from(value.get().year()))
        })
        // A zero year is how an absent one survives some containers.
        .filter(|year| *year > 0);

    let read = MediaTags {
        title: tags
            .get::<gst::tags::Title>()
            .and_then(|value| tag_text(value.get())),
        artist: tags
            .get::<gst::tags::Artist>()
            .and_then(|value| tag_text(value.get())),
        album: tags
            .get::<gst::tags::Album>()
            .and_then(|value| tag_text(value.get())),
        album_artist: tags
            .get::<gst::tags::AlbumArtist>()
            .and_then(|value| tag_text(value.get())),
        composer: tags
            .get::<gst::tags::Composer>()
            .and_then(|value| tag_text(value.get())),
        genre: tags
            .get::<gst::tags::Genre>()
            .and_then(|value| tag_text(value.get())),
        track_number: tags
            .get::<gst::tags::TrackNumber>()
            .map(|value| value.get())
            .filter(|number| *number > 0),
        year,
        encoder: tags
            .get::<gst::tags::Encoder>()
            .and_then(|value| tag_text(value.get())),
    };

    (!read.is_empty()).then_some(read)
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
            tags: info.tags().as_ref().and_then(read_tags),
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

    /// Bodies commonly repeat the make inside the model, and "NIKON CORPORATION NIKON D850"
    /// is not how anyone says it.
    #[test]
    fn folds_a_repeated_make_into_the_model() {
        let fold =
            |make: &str, model: &str| camera_name(Some(make.to_string()), Some(model.to_string()));

        assert_eq!(
            fold("NIKON CORPORATION", "NIKON D850").as_deref(),
            Some("NIKON D850")
        );
        assert_eq!(
            fold("Canon", "Canon EOS R5").as_deref(),
            Some("Canon EOS R5")
        );
        assert_eq!(
            fold("Apple", "iPhone 15 Pro").as_deref(),
            Some("Apple iPhone 15 Pro")
        );
    }

    #[test]
    fn names_a_camera_from_whichever_half_the_file_states() {
        assert_eq!(
            camera_name(Some("Canon".into()), None).as_deref(),
            Some("Canon")
        );
        assert_eq!(
            camera_name(None, Some("EOS R5".into())).as_deref(),
            Some("EOS R5")
        );
        assert_eq!(camera_name(None, None), None);
    }

    #[test]
    fn converts_a_coordinate_from_degrees_minutes_and_seconds() {
        // 37° 46' 29.7", the classic San Francisco latitude.
        let parts = [
            exif::Rational { num: 37, denom: 1 },
            exif::Rational { num: 46, denom: 1 },
            exif::Rational {
                num: 297,
                denom: 10,
            },
        ];

        let degrees = dms_to_degrees(&parts).expect("a coordinate");

        assert!((degrees - 37.774_917).abs() < 0.000_01, "{degrees}");
    }

    #[test]
    fn a_truncated_coordinate_is_not_a_position() {
        assert_eq!(
            dms_to_degrees(&[exif::Rational { num: 37, denom: 1 }]),
            None
        );
        assert_eq!(dms_to_degrees(&[]), None);
    }

    /// South and west are the negative halves. A file that states no hemisphere gets no guess.
    #[test]
    fn signs_a_coordinate_by_its_hemisphere() {
        assert_eq!(apply_hemisphere(37.5, Some("N"), 'S'), 37.5);
        assert_eq!(apply_hemisphere(37.5, Some("S"), 'S'), -37.5);
        assert_eq!(apply_hemisphere(122.4, Some("W"), 'W'), -122.4);
        assert_eq!(apply_hemisphere(122.4, Some("w"), 'W'), -122.4);
        assert_eq!(apply_hemisphere(122.4, None, 'W'), 122.4);
    }

    /// EXIF strings arrive quoted, NUL-terminated and padded, none of which belongs on screen.
    #[test]
    fn strips_what_exif_wraps_its_strings_in() {
        assert_eq!(
            exif_text("\"Canon EOS R5\"".into()).as_deref(),
            Some("Canon EOS R5")
        );
        assert_eq!(
            exif_text("  Lightroom  ".into()).as_deref(),
            Some("Lightroom")
        );
        assert_eq!(exif_text("\"\"".into()), None);
        assert_eq!(exif_text("   ".into()), None);
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
    fn reads_the_tags_a_file_carries() {
        gst::init().expect("gstreamer starts");

        let mut list = gst::TagList::new();
        {
            let list = list.get_mut().expect("a fresh list is writable");
            list.add::<gst::tags::Title>(&"Plastic Love", gst::TagMergeMode::Append);
            list.add::<gst::tags::Artist>(&"Mariya Takeuchi", gst::TagMergeMode::Append);
            list.add::<gst::tags::TrackNumber>(&3, gst::TagMergeMode::Append);
            // Present but blank, which says no more than absent and must not draw a row.
            list.add::<gst::tags::Genre>(&"   ", gst::TagMergeMode::Append);
        }

        let tags = read_tags(&list).expect("tags were read");

        assert_eq!(tags.title.as_deref(), Some("Plastic Love"));
        assert_eq!(tags.artist.as_deref(), Some("Mariya Takeuchi"));
        assert_eq!(tags.track_number, Some(3));
        assert_eq!(tags.genre, None);
        assert_eq!(tags.album, None);
    }

    /// Most files carry nothing, and an object full of nulls would still draw a heading.
    #[test]
    fn a_file_carrying_no_tags_reports_none_at_all() {
        gst::init().expect("gstreamer starts");

        assert!(read_tags(&gst::TagList::new()).is_none());
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
