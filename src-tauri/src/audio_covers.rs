// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Reads cover art embedded in audio files.
//!
//! The video thumbnailer cannot be reused for this: pointed at an MP3 carrying artwork,
//! playbin reports "Video produced no frame", because an attached picture is exposed as a
//! tag rather than as a decodable video track. So the picture is taken from the tag list
//! instead, which also covers FLAC, Ogg and MP4 without format-specific parsing.
//!
//! GStreamer is used rather than a tag-reading crate for the same reason the video
//! thumbnailer uses it: WebKitGTK already requires it, so this adds no runtime dependency.

use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;

/// A damaged or very slow file must not pin a worker thread.
const TAG_SCAN_TIMEOUT: Duration = Duration::from_secs(10);

/// Returns the encoded picture bytes (JPEG or PNG, as stored) or `None` when the file has no
/// artwork. A file that simply carries no cover is not an error.
pub fn extract_embedded_cover(path: &str) -> Result<Option<Vec<u8>>, String> {
    // Safe to call repeatedly; GStreamer guards against re-initialization internally.
    gst::init().map_err(|error| format!("Failed to initialize GStreamer: {error}"))?;

    let uri = gst::glib::filename_to_uri(path, None)
        .map_err(|error| format!("Failed to build audio URI: {error}"))?;

    let playbin = gst::ElementFactory::make("playbin")
        .property("uri", uri.as_str())
        .build()
        .map_err(|error| format!("Failed to create audio pipeline: {error}"))?;

    // Nothing is rendered; the pipeline only needs to get far enough to publish its tags.
    for sink in ["audio-sink", "video-sink"] {
        playbin.set_property(
            sink,
            gst::ElementFactory::make("fakesink")
                .build()
                .map_err(|error| format!("Failed to create {sink}: {error}"))?,
        );
    }

    let result = scan_for_cover(&playbin);

    // Tear the pipeline down on every path, including the error ones.
    let _ = playbin.set_state(gst::State::Null);

    result
}

fn scan_for_cover(playbin: &gst::Element) -> Result<Option<Vec<u8>>, String> {
    // PAUSED is enough: tags are published while the pipeline prerolls.
    playbin
        .set_state(gst::State::Paused)
        .map_err(|error| format!("Failed to start audio pipeline: {error}"))?;

    let bus = playbin
        .bus()
        .ok_or_else(|| "Audio pipeline has no bus".to_string())?;

    let deadline = Instant::now() + TAG_SCAN_TIMEOUT;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());

        if remaining.is_zero() {
            return Ok(None);
        }

        let Some(message) = bus.timed_pop(gst::ClockTime::from_nseconds(
            remaining.as_nanos().min(u128::from(u64::MAX)) as u64,
        )) else {
            return Ok(None);
        };

        match message.view() {
            gst::MessageView::Tag(tag_message) => {
                if let Some(cover) = cover_from_tags(&tag_message.tags()) {
                    return Ok(Some(cover));
                }
            }
            // Preroll finished with no picture among the tags, so there is none to find.
            gst::MessageView::AsyncDone(..) | gst::MessageView::Eos(..) => return Ok(None),
            gst::MessageView::Error(error) => {
                return Err(format!("Failed to read audio tags: {}", error.error()));
            }
            _ => {}
        }
    }
}

fn cover_from_tags(tags: &gst::TagList) -> Option<Vec<u8>> {
    // `Image` is the front-cover style tag; `PreviewImage` is the small variant some files
    // carry instead, and is better than falling back to a generic icon.
    let sample = tags
        .get::<gst::tags::Image>()
        .map(|value| value.get().clone())
        .or_else(|| {
            tags.get::<gst::tags::PreviewImage>()
                .map(|value| value.get().clone())
        })?;

    let buffer = sample.buffer()?;
    let map = buffer.map_readable().ok()?;
    let bytes = map.as_slice();

    if bytes.is_empty() {
        return None;
    }

    Some(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a real audio file, so it is opt-in. Run with:
    /// `SFM_TEST_AUDIO=/path/to.mp3 SFM_TEST_OUT=/tmp/cover.jpg cargo test --lib -- --ignored`
    #[test]
    #[ignore]
    fn extracts_a_real_embedded_cover() {
        let path = std::env::var("SFM_TEST_AUDIO").expect("SFM_TEST_AUDIO must be set");
        let out = std::env::var("SFM_TEST_OUT").expect("SFM_TEST_OUT must be set");

        let cover = extract_embedded_cover(&path)
            .expect("extraction succeeds")
            .expect("file carries a cover");

        assert!(cover.len() > 1024, "cover looks too small to be an image");
        std::fs::write(&out, &cover).expect("cover is written");
    }
}
