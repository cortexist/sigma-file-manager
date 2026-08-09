// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Decodes video frames outside the webview on Linux.
//!
//! WebKitGTK decodes video into a GPU buffer that JavaScript cannot sample, so a frame
//! grabbed with `drawImage(video)`, `createImageBitmap(video)` or WebGL `texImage2D` comes
//! back as uninitialized memory. Only turning off the webview's accelerated video path
//! makes it readable, and that costs accelerated playback and breaks fullscreen video.
//!
//! Decoding here with GStreamer sidesteps the trade-off: the webview keeps full hardware
//! acceleration and neither thumbnails nor still capture depend on WebKit behaviour.
//! GStreamer is already a runtime requirement of WebKitGTK, so this adds no new runtime
//! dependency.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use gstreamer_video::prelude::*;
use gstreamer_video::{VideoFrameRef, VideoInfo};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{imageops, ImageEncoder, RgbImage};

/// Where the frame is taken from, matching the previous webview behaviour.
const THUMBNAIL_POSITION_RATIO: f64 = 0.1;
const THUMBNAIL_POSITION_MAX_SECONDS: f64 = 1.0;

/// A broken or very slow file must not pin a worker thread indefinitely.
const STATE_CHANGE_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(10);
const PULL_TIMEOUT: Duration = Duration::from_secs(10);
/// The retry only waits on a frame the pipeline is already holding, so it needs no patience.
const RETRY_PULL_TIMEOUT: Duration = Duration::from_secs(2);

const JPEG_QUALITY: u8 = 80;

struct DecodedFrame {
    width: u32,
    height: u32,
    /// Tightly packed RGB, three bytes per pixel.
    pixels: Vec<u8>,
}

/// Which frame of the file to pull out.
enum FramePosition {
    /// Near the start, wherever the nearest keyframe happens to be. Good enough for a
    /// thumbnail, and it avoids decoding forward from a keyframe for every file in a folder.
    Thumbnail,
    /// The frame showing at this offset in seconds, decoded from the preceding keyframe so
    /// the still matches what the player had on screen rather than the last keyframe before
    /// it, which can be seconds earlier.
    Exact(f64),
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

fn decode_frame(path: &str, position: FramePosition) -> Result<DecodedFrame, String> {
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

    // Installed before the pipeline runs, so no tag message is posted unseen.
    let cover = watch_for_cover(&playbin);
    let result = capture_frame(&playbin, &app_sink, position, &cover);

    // Tear the pipeline down on every path, including the error ones.
    let _ = playbin.set_state(gst::State::Null);

    result
}

struct SeekTarget {
    seconds: f64,
    flags: gst::SeekFlags,
}

/// Works out where to seek, and how precisely, for a position and a possibly unknown duration.
///
/// `frame_seconds` is how long one frame lasts, which is what separates the last frame's own
/// timestamp from the end of the file. Seeking into that gap lands past every frame there is
/// and the pipeline then has nothing at all to hand over, so an exact capture stops short of
/// it — otherwise pausing on the closing frame of a clip would copy nothing.
///
/// Kept apart from the pipeline so the arithmetic can be tested without GStreamer plugins,
/// which CI does not necessarily have.
fn seek_target(
    position: &FramePosition,
    duration_seconds: Option<f64>,
    frame_seconds: f64,
) -> Option<SeekTarget> {
    match position {
        FramePosition::Thumbnail => {
            // Without a duration there is nothing to take a fraction of, and the frame the
            // pipeline is already holding is the one a thumbnail wants anyway.
            let duration = duration_seconds?;

            Some(SeekTarget {
                seconds: (duration * THUMBNAIL_POSITION_RATIO).min(THUMBNAIL_POSITION_MAX_SECONDS),
                flags: gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            })
        }
        FramePosition::Exact(seconds) => {
            if !seconds.is_finite() {
                return None;
            }

            // A live or still-growing stream reports no duration; take the caller's word for
            // the position there rather than refusing to capture at all.
            let clamped = match duration_seconds {
                Some(duration) if duration > 0.0 => {
                    seconds.clamp(0.0, (duration - frame_seconds).max(0.0))
                }
                _ => seconds.max(0.0),
            };

            Some(SeekTarget {
                seconds: clamped,
                flags: gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            })
        }
    }
}

/// How long one frame of the prerolled video lasts.
///
/// Falls back to a tenth of a second when the file does not declare a frame rate, which is
/// one frame at 10 fps — well below anything real video uses, so the clamp above stays safe.
fn frame_duration_seconds(app_sink: &AppSink) -> f64 {
    const UNKNOWN_FRAME_SECONDS: f64 = 0.1;

    app_sink
        .static_pad("sink")
        .and_then(|pad| pad.current_caps())
        .and_then(|caps| VideoInfo::from_caps(&caps).ok())
        .map(|info| info.fps())
        .filter(|fps| fps.numer() > 0 && fps.denom() > 0)
        .map_or(UNKNOWN_FRAME_SECONDS, |fps| {
            f64::from(fps.denom()) / f64::from(fps.numer())
        })
}

/// How long the video track runs, which is not always how long the file runs: a clip whose
/// audio continues past the last picture reports the longer of the two at the pipeline, and
/// seeking into that tail lands past every frame there is. Asking the branch that carries the
/// video gets the answer that matters, where the demuxer tracks it per stream.
fn video_duration_seconds(playbin: &gst::Element, app_sink: &AppSink) -> Option<f64> {
    let to_seconds = |duration: gst::ClockTime| duration.nseconds() as f64 / 1_000_000_000.0;

    app_sink
        .static_pad("sink")
        .and_then(|pad| pad.query_duration::<gst::ClockTime>())
        .map(to_seconds)
        .or_else(|| playbin.query_duration::<gst::ClockTime>().map(to_seconds))
}

fn seek_and_preroll(playbin: &gst::Element, seek: &SeekTarget) {
    // Best effort throughout: a frame from the start beats no frame at all.
    let _ = playbin.seek_simple(
        seek.flags,
        gst::ClockTime::from_nseconds((seek.seconds * 1_000_000_000.0) as u64),
    );
    let _ = playbin.state(STATE_CHANGE_TIMEOUT);
}

/// Decodes cover bytes into the packed RGB a decoded frame produces, so the two sources
/// share every step after this one.
///
/// The encoding varies by container - PNG from an attached picture track, the WebP
/// Matroska tends to carry - and is read from the bytes rather than trusted from the tag.
fn cover_frame_from_bytes(bytes: &[u8]) -> Option<DecodedFrame> {
    let cover = image::load_from_memory(bytes).ok()?.to_rgb8();
    let (width, height) = (cover.width(), cover.height());

    if width == 0 || height == 0 {
        return None;
    }

    Some(DecodedFrame {
        width,
        height,
        pixels: cover.into_raw(),
    })
}

/// The cover carried by a tag message, if that message is the one carrying it.
///
/// Demuxers all hand it over the same way: qtdemux from an attached picture track,
/// matroskademux from a cover attachment, the mp3 and mp4 parsers from their own boxes.
/// Several tag messages arrive, one per stream, and only one of them has the picture.
fn cover_bytes(tags: &gst::TagList) -> Option<Vec<u8>> {
    let sample = tags
        .get::<gst::tags::Image>()
        .map(|tag| tag.get())
        .or_else(|| tags.get::<gst::tags::PreviewImage>().map(|tag| tag.get()))?;

    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;

    Some(map.as_slice().to_vec())
}

/// Starts collecting the file's cover, keeping the first one that turns up.
///
/// A sync handler rather than a read of the bus afterwards, because when the tag is posted
/// is not ours to decide: it can land after the state change completes, and polling for it
/// would mean waiting on every file that has no cover at all to find out it has none. This
/// sees each message as it is posted and costs nothing to leave running.
///
/// Must be installed before the pipeline starts, or the early messages are already gone.
fn watch_for_cover(playbin: &gst::Element) -> Arc<Mutex<Option<Vec<u8>>>> {
    let found: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

    if let Some(bus) = playbin.bus() {
        let sink = found.clone();

        bus.set_sync_handler(move |_, message| {
            if let gst::MessageView::Tag(message) = message.view() {
                if let Ok(mut slot) = sink.lock() {
                    if slot.is_none() {
                        *slot = cover_bytes(&message.tags());
                    }
                }
            }

            // Everything still reaches the bus; this handler only watches.
            gst::BusSyncReply::Pass
        });
    }

    found
}

fn collected_cover(cover: &Mutex<Option<Vec<u8>>>) -> Option<DecodedFrame> {
    let bytes = cover.lock().ok()?.take()?;
    cover_frame_from_bytes(&bytes)
}

fn capture_frame(
    playbin: &gst::Element,
    app_sink: &AppSink,
    position: FramePosition,
    cover: &Mutex<Option<Vec<u8>>>,
) -> Result<DecodedFrame, String> {
    // A picture the file carries beats any frame guessed out of it: it is what whoever made
    // the file chose to stand for it, where a frame near the start is whatever the nearest
    // keyframe happens to be - the black one a fade-in opens on, as often as not.
    //
    // Thumbnails only. A still capture is a request for one particular moment, and the
    // cover is not that moment.
    let wants_cover = matches!(position, FramePosition::Thumbnail);

    playbin
        .set_state(gst::State::Paused)
        .map_err(|error| format!("Failed to open video: {error}"))?;

    // PAUSED completes once the first frame is decoded and held, which is what we sample.
    if let (Err(error), _, _) = playbin.state(STATE_CHANGE_TIMEOUT) {
        return Err(format!("Video did not open in time: {error}"));
    }

    if wants_cover {
        if let Some(cover) = collected_cover(cover) {
            return Ok(cover);
        }
    }

    let target = seek_target(
        &position,
        video_duration_seconds(playbin, app_sink),
        frame_duration_seconds(app_sink),
    );

    if let Some(target) = &target {
        seek_and_preroll(playbin, target);
    }

    // Seeking and prerolling gave the demuxer more time, so a cover that had not been
    // posted when the state change finished has certainly arrived by now. Cheaper to look
    // twice than to wait for one that may not exist.
    let prefer_cover = |sample: &gst::Sample| -> Result<DecodedFrame, String> {
        if wants_cover {
            if let Some(cover) = collected_cover(cover) {
                return Ok(cover);
            }
        }

        sample_to_frame(sample)
    };

    if let Some(sample) =
        app_sink.try_pull_preroll(gst::ClockTime::from_nseconds(PULL_TIMEOUT.as_nanos() as u64))
    {
        return prefer_cover(&sample);
    }

    // Nothing came back, which past the last frame happens at once: the pipeline is sitting
    // at end-of-stream with no picture to hand over. However the position ended up out there
    // — a duration the file overstates, a stream that ends early — the keyframe at or before
    // it is one that certainly exists, so take that rather than give up on the capture.
    let Some(target) = target else {
        return Err("Video produced no frame".to_string());
    };

    seek_and_preroll(
        playbin,
        &SeekTarget {
            seconds: target.seconds,
            flags: gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::SNAP_BEFORE,
        },
    );

    let sample = app_sink
        .try_pull_preroll(gst::ClockTime::from_nseconds(
            RETRY_PULL_TIMEOUT.as_nanos() as u64
        ))
        .ok_or_else(|| "Video produced no frame".to_string())?;

    prefer_cover(&sample)
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

/// Scales to fit inside `max_width` by `max_height`, keeping the picture's own shape.
///
/// Deliberately no cropping. The card these appear on fills its tile with `object-fit:
/// cover`, so it crops to its own shape regardless; cropping here as well would throw the
/// edges away before anything had the chance to use them. That second crop is what made a
/// 16:9 cover look zoomed in next to the very same picture on an audio file, which reaches
/// the card uncropped.
///
/// A picture already smaller than the bounds is left alone rather than blown up, since the
/// card can scale it further at no cost in stored bytes.
fn fit_within(frame: DecodedFrame, max_width: u32, max_height: u32) -> RgbImage {
    let image = RgbImage::from_raw(frame.width, frame.height, frame.pixels)
        .expect("frame buffer matches its dimensions");

    let scale = f64::min(
        max_width as f64 / frame.width as f64,
        max_height as f64 / frame.height as f64,
    );

    if scale >= 1.0 {
        return image;
    }

    let width = ((frame.width as f64 * scale).round() as u32).max(1);
    let height = ((frame.height as f64 * scale).round() as u32).max(1);

    imageops::resize(&image, width, height, imageops::FilterType::Triangle)
}

/// The picture standing for `path`, as a JPEG bounded by the requested size.
///
/// Its own cover when it has one, a frame near the start when it does not. The size is a
/// bound rather than an exact shape: see `fit_within`.
pub fn generate_video_thumbnail_jpeg(
    path: &str,
    target_width: u32,
    target_height: u32,
) -> Result<Vec<u8>, String> {
    if target_width == 0 || target_height == 0 {
        return Err("Video thumbnail dimensions are invalid".to_string());
    }

    let frame = decode_frame(path, FramePosition::Thumbnail)?;
    let thumbnail = fit_within(frame, target_width, target_height);

    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
        .encode_image(&thumbnail)
        .map_err(|error| format!("Failed to encode video thumbnail: {error}"))?;

    Ok(encoded)
}

/// Decodes the frame showing at `position_seconds` and encodes it as a PNG.
///
/// Unlike a thumbnail this keeps the file's own resolution and takes a lossless encoding,
/// because the point of a still capture is to get the actual picture back out.
pub fn capture_video_frame_png(path: &str, position_seconds: f64) -> Result<Vec<u8>, String> {
    let frame = decode_frame(path, FramePosition::Exact(position_seconds))?;
    let mut encoded = Vec::new();

    PngEncoder::new(&mut encoded)
        .write_image(
            &frame.pixels,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|error| format!("Failed to encode the captured video frame: {error}"))?;

    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card crops to its own shape; arriving with the whole picture is the point.
    #[test]
    fn a_wide_picture_keeps_its_shape() {
        let frame = DecodedFrame {
            width: 1280,
            height: 720,
            pixels: vec![7u8; 1280 * 720 * 3],
        };

        let fitted = fit_within(frame, 384, 271);

        assert_eq!((fitted.width(), fitted.height()), (384, 216));
    }

    #[test]
    fn a_tall_picture_is_bounded_by_its_height() {
        let frame = DecodedFrame {
            width: 40,
            height: 300,
            pixels: vec![3u8; 40 * 300 * 3],
        };

        let fitted = fit_within(frame, 128, 96);

        assert_eq!(fitted.height(), 96);
        assert!(fitted.width() <= 128);
    }

    /// Scaling a small cover up would cost bytes for pixels it does not have.
    #[test]
    fn a_small_picture_is_left_alone() {
        let frame = DecodedFrame {
            width: 64,
            height: 48,
            pixels: vec![1u8; 64 * 48 * 3],
        };

        let fitted = fit_within(frame, 384, 271);

        assert_eq!((fitted.width(), fitted.height()), (64, 48));
    }

    #[test]
    fn a_cover_becomes_a_frame() {
        let cover = RgbImage::from_pixel(8, 4, image::Rgb([10, 20, 30]));
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(cover.as_raw(), 8, 4, image::ExtendedColorType::Rgb8)
            .expect("cover encodes");

        let frame = cover_frame_from_bytes(&png).expect("cover decodes");

        assert_eq!((frame.width, frame.height), (8, 4));
        // Packed RGB, exactly as a decoded frame arrives.
        assert_eq!(frame.pixels.len(), 8 * 4 * 3);
        assert_eq!(&frame.pixels[..3], &[10, 20, 30]);
    }

    /// Nothing usable means no cover, not a failed thumbnail: the keyframe still follows.
    #[test]
    fn an_unreadable_cover_is_no_cover() {
        assert!(cover_frame_from_bytes(b"not an image at all").is_none());
        assert!(cover_frame_from_bytes(&[]).is_none());
    }

    #[test]
    fn rejects_zero_dimensions() {
        assert!(generate_video_thumbnail_jpeg("/tmp/whatever.mp4", 0, 64).is_err());
    }

    #[test]
    fn thumbnails_take_the_nearest_keyframe_near_the_start() {
        let target =
            seek_target(&FramePosition::Thumbnail, Some(300.0), 0.04).expect("seek target");

        assert!(target.flags.contains(gst::SeekFlags::KEY_UNIT));
        assert_eq!(target.seconds, THUMBNAIL_POSITION_MAX_SECONDS);
    }

    /// The still has to be the frame that was on screen, not the keyframe before it.
    #[test]
    fn a_still_capture_seeks_accurately_to_the_asked_for_position() {
        let target =
            seek_target(&FramePosition::Exact(42.5), Some(300.0), 0.04).expect("seek target");

        assert!(target.flags.contains(gst::SeekFlags::ACCURATE));
        assert!(!target.flags.contains(gst::SeekFlags::KEY_UNIT));
        assert_eq!(target.seconds, 42.5);
    }

    #[test]
    fn a_still_capture_stays_inside_the_file() {
        // Stopping a whole frame short of the end is what keeps the last frame reachable.
        let past_end =
            seek_target(&FramePosition::Exact(500.0), Some(300.0), 0.04).expect("seek target");
        assert!((past_end.seconds - 299.96).abs() < 1e-9);

        let negative =
            seek_target(&FramePosition::Exact(-5.0), Some(300.0), 0.04).expect("seek target");
        assert_eq!(negative.seconds, 0.0);
    }

    /// A stream with no reported duration still has a frame on screen worth capturing.
    #[test]
    fn a_still_capture_works_without_a_known_duration() {
        let target = seek_target(&FramePosition::Exact(12.0), None, 0.04).expect("seek target");

        assert_eq!(target.seconds, 12.0);
        assert!(seek_target(&FramePosition::Thumbnail, None, 0.04).is_none());
        assert!(seek_target(&FramePosition::Exact(f64::NAN), None, 0.04).is_none());
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

    /// Manual check for the same reason as above. Run with:
    /// `SFM_TEST_VIDEO=/path/to.mp4 SFM_TEST_POSITION=12.5 SFM_TEST_OUT=/tmp/out.png \
    ///  cargo test --lib -- --ignored capture_a_real_video_frame`
    #[test]
    #[ignore]
    fn capture_a_real_video_frame() {
        let path = std::env::var("SFM_TEST_VIDEO").expect("SFM_TEST_VIDEO must be set");
        let out = std::env::var("SFM_TEST_OUT").expect("SFM_TEST_OUT must be set");
        let position: f64 = std::env::var("SFM_TEST_POSITION")
            .unwrap_or_else(|_| "1.0".to_string())
            .parse()
            .expect("SFM_TEST_POSITION must be a number");

        let png = capture_video_frame_png(&path, position).expect("frame decodes");

        assert!(
            png.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
            "output is not a PNG"
        );
        std::fs::write(&out, &png).expect("frame is written");
    }
}
