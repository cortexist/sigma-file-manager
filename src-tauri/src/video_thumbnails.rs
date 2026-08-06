// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Decodes video thumbnails outside the webview on Linux.
//!
//! WebKitGTK decodes video into a GPU buffer that JavaScript cannot sample, so a frame
//! grabbed with `drawImage(video)`, `createImageBitmap(video)` or WebGL `texImage2D` comes
//! back as uninitialized memory. Only turning off the webview's accelerated video path
//! makes it readable, and that costs accelerated playback and breaks fullscreen video.
//!
//! Decoding here with GStreamer sidesteps the trade-off: the webview keeps full hardware
//! acceleration and thumbnails stop depending on WebKit behaviour. GStreamer is already a
//! runtime requirement of WebKitGTK, so this adds no new runtime dependency.

use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use gstreamer_video::prelude::*;
use gstreamer_video::{VideoFrameRef, VideoInfo};
use image::codecs::jpeg::JpegEncoder;
use image::{imageops, RgbImage};

/// Where the frame is taken from, matching the previous webview behaviour.
const THUMBNAIL_POSITION_RATIO: f64 = 0.1;
const THUMBNAIL_POSITION_MAX_SECONDS: f64 = 1.0;

/// A broken or very slow file must not pin a worker thread indefinitely.
const STATE_CHANGE_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(10);
const PULL_TIMEOUT: Duration = Duration::from_secs(10);

const JPEG_QUALITY: u8 = 80;

struct DecodedFrame {
    width: u32,
    height: u32,
    /// Tightly packed RGB, three bytes per pixel.
    pixels: Vec<u8>,
}

fn build_sink_bin(app_sink: &AppSink) -> Result<gst::Bin, String> {
    let make = |name: &str| {
        gst::ElementFactory::make(name)
            .build()
            .map_err(|error| format!("Failed to create {name}: {error}"))
    };

    // `videoflip method=automatic` applies the rotation tag phone cameras set, so portrait
    // videos are not thumbnailed sideways.
    let flip = gst::ElementFactory::make("videoflip")
        .property_from_str("method", "automatic")
        .build()
        .map_err(|error| format!("Failed to create videoflip: {error}"))?;
    let convert = make("videoconvert")?;
    let scale = make("videoscale")?;

    let bin = gst::Bin::new();
    let sink_element = app_sink.upcast_ref::<gst::Element>();

    bin.add_many([&flip, &convert, &scale, sink_element])
        .map_err(|error| format!("Failed to assemble thumbnail sink: {error}"))?;
    gst::Element::link_many([&flip, &convert, &scale, sink_element])
        .map_err(|error| format!("Failed to link thumbnail sink: {error}"))?;

    let sink_pad = flip
        .static_pad("sink")
        .ok_or_else(|| "Thumbnail sink has no input pad".to_string())?;
    let ghost_pad = gst::GhostPad::with_target(&sink_pad)
        .map_err(|error| format!("Failed to expose thumbnail sink: {error}"))?;
    bin.add_pad(&ghost_pad)
        .map_err(|error| format!("Failed to attach thumbnail sink pad: {error}"))?;

    Ok(bin)
}

fn decode_frame(path: &str) -> Result<DecodedFrame, String> {
    // Safe to call repeatedly; GStreamer guards against re-initialization internally.
    gst::init().map_err(|error| format!("Failed to initialize GStreamer: {error}"))?;

    let uri = gst::glib::filename_to_uri(path, None)
        .map_err(|error| format!("Failed to build video URI: {error}"))?;

    let playbin = gst::ElementFactory::make("playbin")
        .property("uri", uri.as_str())
        .build()
        .map_err(|error| format!("Failed to create video pipeline: {error}"))?;

    // Packed RGB at square pixels, so anamorphic sources are corrected before we sample.
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGB")
        .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
        .build();

    let app_sink = AppSink::builder()
        .caps(&caps)
        .max_buffers(1)
        .drop(true)
        .sync(false)
        .build();

    playbin.set_property("video-sink", build_sink_bin(&app_sink)?);
    // Audio is irrelevant here, and a missing audio sink must not fail the pipeline.
    playbin.set_property(
        "audio-sink",
        gst::ElementFactory::make("fakesink")
            .build()
            .map_err(|error| format!("Failed to create audio sink: {error}"))?,
    );

    let result = capture_frame(&playbin, &app_sink);

    // Tear the pipeline down on every path, including the error ones.
    let _ = playbin.set_state(gst::State::Null);

    result
}

fn capture_frame(playbin: &gst::Element, app_sink: &AppSink) -> Result<DecodedFrame, String> {
    playbin
        .set_state(gst::State::Paused)
        .map_err(|error| format!("Failed to open video: {error}"))?;

    // PAUSED completes once the first frame is decoded and held, which is what we sample.
    if let (Err(error), _, _) = playbin.state(STATE_CHANGE_TIMEOUT) {
        return Err(format!("Video did not open in time: {error}"));
    }

    if let Some(duration) = playbin.query_duration::<gst::ClockTime>() {
        let seconds = duration.nseconds() as f64 / 1_000_000_000.0;
        let position = (seconds * THUMBNAIL_POSITION_RATIO).min(THUMBNAIL_POSITION_MAX_SECONDS);

        // Best effort: a frame from the start beats no thumbnail at all.
        let _ = playbin.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::ClockTime::from_nseconds((position * 1_000_000_000.0) as u64),
        );
        let _ = playbin.state(STATE_CHANGE_TIMEOUT);
    }

    let sample = app_sink
        .try_pull_preroll(gst::ClockTime::from_nseconds(PULL_TIMEOUT.as_nanos() as u64))
        .ok_or_else(|| "Video produced no frame".to_string())?;

    sample_to_frame(&sample)
}

fn sample_to_frame(sample: &gst::Sample) -> Result<DecodedFrame, String> {
    let caps = sample
        .caps()
        .ok_or_else(|| "Decoded frame has no format".to_string())?;
    let info =
        VideoInfo::from_caps(caps).map_err(|error| format!("Decoded frame format: {error}"))?;
    let buffer = sample
        .buffer()
        .ok_or_else(|| "Decoded frame has no data".to_string())?;

    let frame = VideoFrameRef::from_buffer_ref_readable(buffer, &info)
        .map_err(|error| format!("Failed to read decoded frame: {error}"))?;

    let width = frame.width();
    let height = frame.height();

    if width == 0 || height == 0 {
        return Err("Decoded frame has invalid dimensions".to_string());
    }

    // Rows are padded to an alignment boundary, so copy row by row using the real stride
    // rather than assuming the buffer is tightly packed.
    let stride = frame.plane_stride()[0] as usize;
    let plane = frame
        .plane_data(0)
        .map_err(|error| format!("Decoded frame has no pixels: {error}"))?;
    let row_bytes = width as usize * 3;

    if stride < row_bytes || plane.len() < stride * (height as usize - 1) + row_bytes {
        return Err("Decoded frame is truncated".to_string());
    }

    let mut pixels = Vec::with_capacity(row_bytes * height as usize);

    for row in 0..height as usize {
        let start = row * stride;
        pixels.extend_from_slice(&plane[start..start + row_bytes]);
    }

    Ok(DecodedFrame {
        width,
        height,
        pixels,
    })
}

/// Scales to cover `target_width` by `target_height` and centre-crops the overflow, which
/// is what the webview implementation did with `drawImage`.
fn cover_crop(frame: DecodedFrame, target_width: u32, target_height: u32) -> RgbImage {
    let image = RgbImage::from_raw(frame.width, frame.height, frame.pixels)
        .expect("frame buffer matches its dimensions");

    let scale = f64::max(
        target_width as f64 / frame.width as f64,
        target_height as f64 / frame.height as f64,
    );
    let scaled_width = ((frame.width as f64 * scale).ceil() as u32).max(target_width);
    let scaled_height = ((frame.height as f64 * scale).ceil() as u32).max(target_height);

    let resized = imageops::resize(
        &image,
        scaled_width,
        scaled_height,
        imageops::FilterType::Triangle,
    );

    let offset_x = (scaled_width - target_width) / 2;
    let offset_y = (scaled_height - target_height) / 2;

    imageops::crop_imm(&resized, offset_x, offset_y, target_width, target_height).to_image()
}

/// Decodes a frame from `path` and encodes it as a JPEG of exactly the requested size.
pub fn generate_video_thumbnail_jpeg(
    path: &str,
    target_width: u32,
    target_height: u32,
) -> Result<Vec<u8>, String> {
    if target_width == 0 || target_height == 0 {
        return Err("Video thumbnail dimensions are invalid".to_string());
    }

    let frame = decode_frame(path)?;
    let thumbnail = cover_crop(frame, target_width, target_height);

    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
        .encode_image(&thumbnail)
        .map_err(|error| format!("Failed to encode video thumbnail: {error}"))?;

    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crop_fills_the_target_exactly() {
        let frame = DecodedFrame {
            width: 100,
            height: 50,
            pixels: vec![7u8; 100 * 50 * 3],
        };

        let cropped = cover_crop(frame, 64, 64);

        assert_eq!(cropped.width(), 64);
        assert_eq!(cropped.height(), 64);
    }

    #[test]
    fn cover_crop_handles_tall_sources() {
        let frame = DecodedFrame {
            width: 40,
            height: 300,
            pixels: vec![3u8; 40 * 300 * 3],
        };

        let cropped = cover_crop(frame, 128, 96);

        assert_eq!(cropped.width(), 128);
        assert_eq!(cropped.height(), 96);
    }

    #[test]
    fn rejects_zero_dimensions() {
        assert!(generate_video_thumbnail_jpeg("/tmp/whatever.mp4", 0, 64).is_err());
    }

    /// Manual check against a real file, since decoding needs GStreamer plugins that CI
    /// may not install. Run with:
    /// `SFM_TEST_VIDEO=/path/to.mp4 SFM_TEST_OUT=/tmp/out.jpg cargo test --lib -- --ignored`
    #[test]
    #[ignore]
    fn decodes_a_real_video_file() {
        let path = std::env::var("SFM_TEST_VIDEO").expect("SFM_TEST_VIDEO must be set");
        let out = std::env::var("SFM_TEST_OUT").expect("SFM_TEST_OUT must be set");

        let jpeg = generate_video_thumbnail_jpeg(&path, 384, 271).expect("thumbnail decodes");

        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "output is not a JPEG");
        std::fs::write(&out, &jpeg).expect("thumbnail is written");
    }
}
