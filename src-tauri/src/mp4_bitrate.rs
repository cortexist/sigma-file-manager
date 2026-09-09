// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright © 2026 Cortexist, LLC. All rights reserved.

//! Read-only repair of missing MP4 video bitrate hints for WebKitGTK autoplay.
//! A zero btrt average can wedge HTTP playback during preroll. Derive the hint
//! from sample sizes and track duration, and overlay only those four bytes.
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub offset: u64,
    pub bytes: [u8; 4],
}

#[derive(Clone, Copy)]
struct Atom {
    kind: [u8; 4],
    body: usize,
    end: usize,
}
fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}
fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(
        data.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}
fn atoms(data: &[u8], start: usize, end: usize) -> Option<Vec<Atom>> {
    if end > data.len() || start > end {
        return None;
    }
    let mut result = Vec::new();
    let mut at = start;
    while at < end {
        if end - at < 8 {
            return None;
        }
        let kind = data[at + 4..at + 8].try_into().ok()?;
        let (size, header) = match u32_at(data, at)? {
            0 => ((end - at) as u64, 8),
            1 => (u64_at(data, at + 8)?, 16),
            n => (u64::from(n), 8),
        };
        let size = usize::try_from(size).ok()?;
        let next = at.checked_add(size)?;
        if size < header || next > end {
            return None;
        }
        result.push(Atom {
            kind,
            body: at + header,
            end: next,
        });
        at = next;
    }
    Some(result)
}
fn child(data: &[u8], parent: Atom, kind: &[u8; 4]) -> Option<Atom> {
    atoms(data, parent.body, parent.end)?
        .into_iter()
        .find(|a| &a.kind == kind)
}
fn track_bitrate(data: &[u8], mdhd: Atom, stsz: Atom) -> Option<u32> {
    let header = data.get(mdhd.body..mdhd.end)?;
    let (scale, duration) = match *header.first()? {
        0 => (u32_at(header, 12)?, u64::from(u32_at(header, 16)?)),
        1 => (u32_at(header, 20)?, u64_at(header, 24)?),
        _ => return None,
    };
    if duration == 0 || scale == 0 {
        return None;
    }
    let sizes = data.get(stsz.body..stsz.end)?;
    let fixed = u32_at(sizes, 4)?;
    let count = u32_at(sizes, 8)? as usize;
    let total = if fixed != 0 {
        u64::from(fixed) * count as u64
    } else {
        let end = 12usize.checked_add(count.checked_mul(4)?)?;
        sizes
            .get(12..end)?
            .chunks_exact(4)
            .map(|x| u64::from(u32::from_be_bytes(x.try_into().unwrap())))
            .sum::<u64>()
    };
    let rate =
        u32::try_from(u128::from(total) * 8 * u128::from(scale) / u128::from(duration)).ok()?;
    (rate > 0).then_some(rate)
}
fn parse_moov(data: &[u8], file_offset: u64) -> Vec<Patch> {
    let mut patches = Vec::new();
    for trak in atoms(data, 0, data.len()).unwrap_or_default() {
        if trak.kind != *b"trak" {
            continue;
        }
        let parse = || -> Option<Vec<Patch>> {
            let mdia = child(data, trak, b"mdia")?;
            let hdlr = child(data, mdia, b"hdlr")?;
            if data.get(hdlr.body..hdlr.end)?.get(8..12)? != b"vide" {
                return None;
            }
            let mdhd = child(data, mdia, b"mdhd")?;
            let minf = child(data, mdia, b"minf")?;
            let stbl = child(data, minf, b"stbl")?;
            let rate = track_bitrate(data, mdhd, child(data, stbl, b"stsz")?)?;
            let stsd = child(data, stbl, b"stsd")?;
            let mut found = Vec::new();
            for entry in atoms(data, stsd.body.checked_add(8)?, stsd.end)? {
                // VisualSampleEntry header; leave audio, encrypted and unknown formats alone.
                if !matches!(&entry.kind, b"avc1" | b"avc3" | b"hvc1" | b"hev1") {
                    continue;
                }
                for item in atoms(data, entry.body.checked_add(78)?, entry.end)? {
                    if item.kind == *b"btrt"
                        && item.end - item.body == 12
                        && u32_at(data, item.body + 8)? == 0
                    {
                        found.push(Patch {
                            offset: file_offset + (item.body + 8) as u64,
                            bytes: rate.to_be_bytes(),
                        });
                    }
                }
            }
            Some(found)
        };
        if let Some(found) = parse() {
            patches.extend(found);
        }
    }
    patches
}

/// Only metadata is read; large mdat boxes are skipped. Unrecognized/oversized
/// metadata is left unchanged. Caller seeks to its requested range afterwards.
pub async fn read_patches(file: &mut tokio::fs::File, size: u64) -> Vec<Patch> {
    async fn read(file: &mut tokio::fs::File, size: u64) -> Option<Vec<Patch>> {
        let mut at = 0u64;
        // Bound both allocation and work on untrusted files.
        for _ in 0..4096 {
            if size.checked_sub(at)? < 8 {
                return None;
            }
            file.seek(std::io::SeekFrom::Start(at)).await.ok()?;
            let mut header = [0u8; 8];
            file.read_exact(&mut header).await.ok()?;
            let (length, header_size) = match u32_at(&header, 0)? {
                0 => (size - at, 8),
                1 => {
                    file.read_exact(&mut header[..]).await.ok()?;
                    (u64::from_be_bytes(header), 16)
                }
                n => (u64::from(n), 8),
            };
            if length < header_size || length > size - at {
                return None;
            }
            // Extended-size read overwrites header; fetch kind independently.
            file.seek(std::io::SeekFrom::Start(at + 4)).await.ok()?;
            let mut kind = [0u8; 4];
            file.read_exact(&mut kind).await.ok()?;
            if kind == *b"moov" {
                let payload = length - header_size;
                if payload > 8 * 1024 * 1024 {
                    return None;
                }
                file.seek(std::io::SeekFrom::Start(at + header_size))
                    .await
                    .ok()?;
                let mut data = vec![0; payload as usize];
                file.read_exact(&mut data).await.ok()?;
                return Some(parse_moov(&data, at + header_size));
            }
            at = at.checked_add(length)?;
        }
        None
    }
    read(file, size).await.unwrap_or_default()
}

/// Range requests and stream chunk boundaries can split a four-byte field.
pub fn overlay(chunk: &mut [u8], start: u64, patches: &[Patch]) {
    for patch in patches {
        for (i, byte) in patch.bytes.iter().enumerate() {
            if let Some(offset) = (patch.offset + i as u64).checked_sub(start) {
                if let Some(target) = usize::try_from(offset).ok().and_then(|n| chunk.get_mut(n)) {
                    *target = *byte;
                }
            }
        }
    }
}

pub fn stream(
    file: tokio::fs::File,
    start: u64,
    length: u64,
    patches: Vec<Patch>,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    use futures_util::TryStreamExt;
    let mut position = start;
    tokio_util::io::ReaderStream::new(file.take(length)).map_ok(move |chunk| {
        let count = chunk.len() as u64;
        let result = if patches.iter().any(|p| {
            p.offset < position.saturating_add(count) && p.offset.saturating_add(4) > position
        }) {
            let mut data = chunk.to_vec();
            overlay(&mut data, position, &patches);
            data.into()
        } else {
            chunk
        };
        position += count;
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut result = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        result.extend(kind);
        result.extend(body);
        result
    }
    fn fixture(avg: u32, video: bool, duration: u32) -> Vec<u8> {
        let mut hdlr = vec![0; 8];
        hdlr.extend(if video { b"vide" } else { b"soun" });
        let mut mdhd = vec![0; 12];
        mdhd.extend(1000u32.to_be_bytes());
        mdhd.extend(duration.to_be_bytes());
        let mut sizes = vec![0; 8];
        sizes.extend(2u32.to_be_bytes());
        sizes.extend(100u32.to_be_bytes());
        sizes.extend(300u32.to_be_bytes());
        let mut bitrate = vec![0; 4];
        bitrate.extend(9000u32.to_be_bytes());
        bitrate.extend(avg.to_be_bytes());
        let mut visual = vec![0; 78];
        visual.extend(atom(b"btrt", &bitrate));
        let mut stsd = vec![0; 4];
        stsd.extend(1u32.to_be_bytes());
        stsd.extend(atom(b"avc1", &visual));
        let mut stbl = atom(b"stsd", &stsd);
        stbl.extend(atom(b"stsz", &sizes));
        let mut mdia = atom(b"hdlr", &hdlr);
        mdia.extend(atom(b"mdhd", &mdhd));
        mdia.extend(atom(b"minf", &atom(b"stbl", &stbl)));
        atom(b"trak", &atom(b"mdia", &mdia))
    }
    #[test]
    fn calculates_average_from_samples_without_touching_media() {
        let data = fixture(0, true, 2000);
        let patches = parse_moov(&data, 100);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].bytes, 1600u32.to_be_bytes());
        let mut served = data.clone();
        overlay(&mut served, 100, &patches);
        assert!(parse_moov(&served, 100).is_empty());
        assert_eq!(data.len(), served.len());
        for i in 0..data.len() {
            if !(patches[0].offset..patches[0].offset + 4).contains(&(i as u64 + 100)) {
                assert_eq!(data[i], served[i]);
            }
        }
    }
    #[test]
    fn preserves_known_bitrates_audio_and_unknown_duration() {
        assert!(parse_moov(&fixture(1234, true, 2000), 0).is_empty());
        assert!(parse_moov(&fixture(0, false, 2000), 0).is_empty());
        assert!(parse_moov(&fixture(0, true, 0), 0).is_empty());
    }
    #[test]
    fn ignores_truncated_and_malformed_boxes() {
        let data = fixture(0, true, 2000);
        for end in 0..data.len() {
            assert!(parse_moov(&data[..end], 0).is_empty());
        }
        assert!(parse_moov(&[0, 0, 0, 1, b't', b'r', b'a', b'k'], 0).is_empty());
    }
    #[test]
    fn overlays_partial_ranges_and_split_chunks() {
        let patch = Patch {
            offset: 10,
            bytes: [1, 2, 3, 4],
        };
        let mut whole = vec![0; 20];
        overlay(&mut whole, 0, &[patch.clone()]);
        for start in 0..20 {
            for end in start..=20 {
                let mut range = vec![0; end - start];
                overlay(&mut range, start as u64, &[patch.clone()]);
                assert_eq!(range, whole[start..end]);
            }
        }
    }
    #[tokio::test]
    async fn reads_tail_moov_and_streams_ranges_without_writing() {
        use futures_util::TryStreamExt;
        let temp = tempfile::NamedTempFile::new().unwrap();
        let mut data = atom(b"ftyp", b"isom");
        data.extend(atom(b"mdat", &[7; 23]));
        data.extend(atom(b"moov", &fixture(0, true, 2000)));
        std::fs::write(temp.path(), &data).unwrap();
        let mut file = tokio::fs::File::open(temp.path()).await.unwrap();
        let patches = read_patches(&mut file, data.len() as u64).await;
        assert_eq!(patches.len(), 1);
        let start = patches[0].offset + 1;
        file.seek(std::io::SeekFrom::Start(start)).await.unwrap();
        let chunks: Vec<_> = stream(file, start, 2, patches.clone())
            .try_collect()
            .await
            .unwrap();
        assert_eq!(chunks.concat(), patches[0].bytes[1..3]);
        assert_eq!(std::fs::read(temp.path()).unwrap(), data);
    }
}
