// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Loopback HTTP server used to play local media files in the webview.
//!
//! WebKitGTK hands HTML5 media loading to GStreamer, which has no source element for
//! custom URI schemes, so `<video src="asset://...">` fails with a format error on Linux
//! even though images on the same scheme load fine (WebKit bug 146351). Serving media
//! over `http://127.0.0.1` instead gives the media backend a transport it understands,
//! with real range requests so seeking does not re-read the file from the start.
//!
//! The server binds to loopback only and every request must carry a random per-session
//! token, so other local processes cannot use it to read arbitrary files.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use once_cell::sync::Lazy;
use rand::RngExt;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Mutex;
use tokio_util::io::ReaderStream;

/// Cached origin of the running server, e.g. `http://127.0.0.1:45321/media/<token>`.
static MEDIA_SERVER_ORIGIN: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

struct MediaServerState {
    token: String,
}

#[derive(Deserialize)]
struct MediaQuery {
    path: String,
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Length-independent comparison so a token cannot be recovered byte by byte.
fn tokens_match(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();

    if expected.len() != provided.len() {
        return false;
    }

    let mut difference = 0u8;
    for (left, right) in expected.iter().zip(provided.iter()) {
        difference |= left ^ right;
    }

    difference == 0
}

/// Parses a single-range `bytes=start-end` header into an inclusive byte range.
///
/// Returns `None` when the header is absent or not a form we serve, in which case the
/// caller responds with the whole file. Returns `Some(Err(()))` when the range is
/// syntactically valid but unsatisfiable, which must become a 416.
#[allow(clippy::result_unit_err)]
fn parse_range(headers: &HeaderMap, total_size: u64) -> Option<Result<(u64, u64), ()>> {
    let raw = headers.get(header::RANGE)?.to_str().ok()?;
    let spec = raw.strip_prefix("bytes=")?.trim();

    // Multi-range requests are not worth supporting for media playback.
    if spec.contains(',') {
        return None;
    }

    let (raw_start, raw_end) = spec.split_once('-')?;
    let raw_start = raw_start.trim();
    let raw_end = raw_end.trim();

    if total_size == 0 {
        return Some(Err(()));
    }

    // Suffix range: `bytes=-500` means the final 500 bytes.
    if raw_start.is_empty() {
        let suffix_length: u64 = raw_end.parse().ok()?;
        if suffix_length == 0 {
            return Some(Err(()));
        }
        let start = total_size.saturating_sub(suffix_length);
        return Some(Ok((start, total_size - 1)));
    }

    let start: u64 = raw_start.parse().ok()?;
    if start >= total_size {
        return Some(Err(()));
    }

    let end = if raw_end.is_empty() {
        total_size - 1
    } else {
        match raw_end.parse::<u64>() {
            Ok(value) => value.min(total_size - 1),
            Err(_) => return None,
        }
    };

    if end < start {
        return Some(Err(()));
    }

    Some(Ok((start, end)))
}

fn cors_headers() -> [(header::HeaderName, &'static str); 3] {
    [
        (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        (header::ACCESS_CONTROL_ALLOW_HEADERS, "Range"),
        (
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Content-Length, Accept-Ranges",
        ),
    ]
}

async fn handle_preflight() -> Response {
    (StatusCode::NO_CONTENT, cors_headers()).into_response()
}

async fn serve_media(
    AxumPath(token): AxumPath<String>,
    Query(query): Query<MediaQuery>,
    State(state): State<Arc<MediaServerState>>,
    headers: HeaderMap,
) -> Response {
    if !tokens_match(&state.token, &token) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let requested_path = PathBuf::from(&query.path);
    if !requested_path.is_absolute() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let resolved_path = match dunce::canonicalize(&requested_path) {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let metadata = match tokio::fs::metadata(&resolved_path).await {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return StatusCode::FORBIDDEN.into_response(),
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let total_size = metadata.len();
    let content_type = mime_guess::from_path(&resolved_path)
        .first_or_octet_stream()
        .to_string();

    let mut file = match tokio::fs::File::open(&resolved_path).await {
        Ok(file) => file,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let range = parse_range(&headers, total_size);

    if let Some(Err(())) = range {
        return (
            StatusCode::RANGE_NOT_SATISFIABLE,
            cors_headers(),
            [(header::CONTENT_RANGE, format!("bytes */{total_size}"))],
        )
            .into_response();
    }

    match range {
        Some(Ok((start, end))) => {
            if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }

            let length = end - start + 1;
            let stream = ReaderStream::new(file.take(length));

            (
                StatusCode::PARTIAL_CONTENT,
                cors_headers(),
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::CONTENT_LENGTH, length.to_string()),
                    (
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{total_size}"),
                    ),
                ],
                Body::from_stream(stream),
            )
                .into_response()
        }
        _ => {
            let stream = ReaderStream::new(file);

            (
                StatusCode::OK,
                cors_headers(),
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::CONTENT_LENGTH, total_size.to_string()),
                ],
                Body::from_stream(stream),
            )
                .into_response()
        }
    }
}

/// Starts the loopback media server on first use and returns its tokenized origin.
///
/// The frontend appends `?path=<absolute path>` to build a playable media URL.
pub async fn ensure_media_server() -> Result<String, String> {
    let mut origin_lock = MEDIA_SERVER_ORIGIN.lock().await;

    if let Some(origin) = origin_lock.as_ref() {
        return Ok(origin.clone());
    }

    let token = generate_token();
    let state = Arc::new(MediaServerState {
        token: token.clone(),
    });

    let router = Router::new()
        .route("/media/{token}", get(serve_media).options(handle_preflight))
        .with_state(state);

    // Port 0 lets the OS pick a free ephemeral port; loopback only, never the LAN.
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|err| format!("Failed to bind media server: {err}"))?;

    let port = listener
        .local_addr()
        .map_err(|err| format!("Failed to read media server address: {err}"))?
        .port();

    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, router).await {
            log::error!("Media server stopped: {err}");
        }
    });

    let origin = format!("http://127.0.0.1:{port}/media/{token}");
    log::info!("Media server listening on 127.0.0.1:{port}");
    *origin_lock = Some(origin.clone());

    Ok(origin)
}

#[tauri::command]
pub async fn get_media_server_origin() -> Result<String, String> {
    ensure_media_server().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_range(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, value.parse().unwrap());
        headers
    }

    #[test]
    fn parses_open_ended_range() {
        let headers = headers_with_range("bytes=100-");
        assert_eq!(parse_range(&headers, 1000), Some(Ok((100, 999))));
    }

    #[test]
    fn parses_closed_range() {
        let headers = headers_with_range("bytes=0-1445");
        assert_eq!(parse_range(&headers, 1000), Some(Ok((0, 999))));
    }

    #[test]
    fn parses_suffix_range() {
        let headers = headers_with_range("bytes=-500");
        assert_eq!(parse_range(&headers, 1000), Some(Ok((500, 999))));
    }

    #[test]
    fn rejects_unsatisfiable_start() {
        let headers = headers_with_range("bytes=5000-");
        assert_eq!(parse_range(&headers, 1000), Some(Err(())));
    }

    #[test]
    fn ignores_missing_and_multi_range() {
        assert_eq!(parse_range(&HeaderMap::new(), 1000), None);
        let headers = headers_with_range("bytes=0-10,20-30");
        assert_eq!(parse_range(&headers, 1000), None);
    }

    #[test]
    fn token_comparison_rejects_mismatches() {
        assert!(tokens_match("abc123", "abc123"));
        assert!(!tokens_match("abc123", "abc124"));
        assert!(!tokens_match("abc123", "abc12"));
    }

    /// Exercises the real server over the loopback socket, the same way the webview does.
    #[test]
    fn serves_files_with_range_support() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            let body: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
            let file_path = std::env::temp_dir().join("sfm-media-server-test.bin");
            tokio::fs::write(&file_path, &body).await.unwrap();

            let origin = ensure_media_server().await.unwrap();
            let encoded = urlencoding::encode(&file_path.to_string_lossy()).into_owned();
            let url = format!("{origin}?path={encoded}");
            let client = reqwest::Client::new();

            // Whole file.
            let response = client.get(&url).send().await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["accept-ranges"], "bytes");
            assert_eq!(response.bytes().await.unwrap().as_ref(), body.as_slice());

            // Partial content must return exactly the requested window.
            let response = client
                .get(&url)
                .header(header::RANGE, "bytes=1000-1099")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 206);
            assert_eq!(response.headers()["content-range"], "bytes 1000-1099/4096");
            assert_eq!(
                response.bytes().await.unwrap().as_ref(),
                &body[1000..=1099]
            );

            // Open-ended range runs to the end of the file.
            let response = client
                .get(&url)
                .header(header::RANGE, "bytes=4000-")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 206);
            assert_eq!(response.headers()["content-range"], "bytes 4000-4095/4096");
            assert_eq!(response.bytes().await.unwrap().len(), 96);

            // Past the end of the file.
            let response = client
                .get(&url)
                .header(header::RANGE, "bytes=99999-")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 416);

            // A wrong token must not read the file.
            let base = origin.rsplit_once('/').map(|(base, _)| base).unwrap();
            let response = client
                .get(format!("{base}/{}?path={encoded}", "0".repeat(64)))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 403);

            // Directories and missing files are refused.
            let missing = urlencoding::encode("/nonexistent/sfm/media.mp4").into_owned();
            let response = client
                .get(format!("{origin}?path={missing}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 404);

            tokio::fs::remove_file(&file_path).await.ok();
        });
    }
}
