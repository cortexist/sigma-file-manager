// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

/**
 * Reassembles the metadata the Zune software used to pull from its marketplace out of files the
 * user curates themselves: an `artist.info` sidecar and, failing that, the track filename.
 *
 * The sidecar format is deliberately whatever you get from selecting a Wikipedia infobox and
 * pressing copy, because that is what people actually do. That paste has three shapes to survive:
 *
 *   Released<TAB>28 June 1985[1]     a key and its value on one line, with citation markers
 *   Length<TAB>                      a key whose values continue on the following lines
 *   8:22 (album version)
 *   Mark KnopflerSting               list items concatenated without any separator at all
 *
 * Hand-written files are supported too: `Key: value` is accepted alongside the tab form, and a
 * file with no keys at all is read as free prose.
 */

export interface ArtistInfo {
  /** Lines before the first key, e.g. `Single by Dire Straits`. */
  lead: string[];
  /** Keys lowercased; every key holds a list because infobox values are frequently lists. */
  fields: Record<string, string[]>;
}

export interface NowPlayingCard {
  kicker: string;
  /** Pre-broken display lines. Set in huge type, so the break belongs to the data, not CSS. */
  headline: string[];
  /** One short line under the headline. */
  sub?: string;
  /** Prose that wraps, used for biography fragments instead of `sub`. */
  body?: string;
}

export interface NowPlayingSource {
  info?: ArtistInfo | null;
  /** File name including extension, used only where the sidecar is silent. */
  fileName?: string | null;
}

/**
 * Name particles that legitimately sit before a capital inside a single name. Without these,
 * splitting run-together list items turns `McCartney` into `Mc` + `Cartney`.
 */
const NAME_PARTICLES = new Set([
  'mc', 'mac', 'de', 'del', 'della', 'di', 'da', 'du', 'la', 'le', 'van', 'von', 'st', 'o\'', 'ter', 'ten',
]);

/** Wikipedia citation and note markers: `[1]`, `[a]`, `[note 3]`. */
const CITATION_MARKER = /\[[^\]]{1,12}\]/g;

/** A key in the `Key: value` form. Letters only, so `8:22` stays a value. */
const COLON_KEY = /^([A-Za-z][A-Za-z .'-]{0,23}):\s*(.*)$/;

function clean(value: string): string {
  return value.replace(CITATION_MARKER, '').replace(/\s+/g, ' ').trim();
}

/**
 * Splits list items that lost their separators on the way out of a rendered infobox, where
 * `<li>Mark Knopfler</li><li>Sting</li>` arrives as `Mark KnopflerSting`. The only available
 * signal is a lowercase letter meeting an uppercase one, which is a heuristic rather than a
 * rule: `McCartney` and `DeVito` are guarded by {@link NAME_PARTICLES}, but a genuine run of
 * internal capitals will still be broken. Putting one value per line in the sidecar avoids the
 * guesswork entirely.
 */
export function splitRunTogetherValues(value: string): string[] {
  const parts: string[] = [];
  let start = 0;

  for (let i = 1; i < value.length; i += 1) {
    const previous = value[i - 1];
    const current = value[i];
    const isBoundary = previous >= 'a' && previous <= 'z' && current >= 'A' && current <= 'Z';

    if (!isBoundary) {
      continue;
    }

    const left = value.slice(start, i);
    const lastWord = left.split(' ').pop() ?? '';

    if (NAME_PARTICLES.has(lastWord.toLowerCase())) {
      continue;
    }

    parts.push(left);
    start = i;
  }

  parts.push(value.slice(start));

  return parts.map(part => part.trim()).filter(Boolean);
}

function pushValue(fields: Record<string, string[]>, key: string, rawValue: string) {
  const value = clean(rawValue);

  if (!value) {
    return;
  }

  const existing = fields[key] ?? [];
  fields[key] = existing.concat(splitRunTogetherValues(value));
}

/**
 * Parses an `artist.info` sidecar. Unrecognised lines are never dropped: anything before the
 * first key becomes {@link ArtistInfo.lead}, so a file of plain prose still carries its text
 * through to the show.
 */
export function parseArtistInfo(text: string): ArtistInfo {
  const lead: string[] = [];
  const fields: Record<string, string[]> = {};
  let openKey: string | null = null;

  for (const rawLine of text.split(/\r?\n/)) {
    // Trailing whitespace must survive this far: `Length\t` with no value is precisely how the
    // paste marks a key whose values continue on the lines below, and trimming it first turns
    // that key into a continuation of the key above it.
    const line = rawLine.replace(/\r$/, '');

    if (!line.trim()) {
      continue;
    }

    const tabIndex = line.indexOf('\t');
    const colonMatch = tabIndex === -1 ? COLON_KEY.exec(line.trim()) : null;

    if (tabIndex > 0) {
      openKey = clean(line.slice(0, tabIndex)).toLowerCase();
      pushValue(fields, openKey, line.slice(tabIndex + 1));
      continue;
    }

    if (colonMatch) {
      openKey = clean(colonMatch[1]).toLowerCase();
      pushValue(fields, openKey, colonMatch[2]);
      continue;
    }

    // No key on this line: it either continues the key above it, or — before any key has been
    // seen — it is one of the infobox's opening descriptive lines.
    if (openKey) {
      pushValue(fields, openKey, line);
    }
    else {
      const value = clean(line);

      if (value) {
        lead.push(value);
      }
    }
  }

  return {
    lead,
    fields,
  };
}

/**
 * Breaks a title across at most two lines at the word boundary nearest the middle, which is how
 * the oversized Zune headlines were set. `Money for Nothing` becomes `Money for` / `Nothing`.
 */
export function splitHeadline(text: string): string[] {
  const value = clean(text);
  const words = value.split(' ');

  if (words.length < 2 || value.length <= 12) {
    return value ? [value] : [];
  }

  const middle = value.length / 2;
  let bestIndex = 1;
  let bestDistance = Number.POSITIVE_INFINITY;

  for (let i = 1; i < words.length; i += 1) {
    const width = words.slice(0, i).join(' ').length;
    const distance = Math.abs(width - middle);

    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = i;
    }
  }

  return [words.slice(0, bestIndex).join(' '), words.slice(bestIndex).join(' ')];
}

/**
 * Reads `Artist - Title.mp3`. Downloads routinely carry no tags at all — the file this was
 * written against is a yt-dlp capture with an empty tag list — so the name is often the only
 * place the artist and title exist.
 */
export function parseTrackFileName(fileName: string): {
  artist?: string;
  title?: string;
} {
  const withoutExtension = fileName.replace(/\.[A-Za-z0-9]{1,5}$/, '');
  // Leading track numbers: `01. `, `01 - `, `1 `.
  const withoutTrackNumber = withoutExtension.replace(/^\s*\d{1,3}\s*[.\-–—]?\s+/, '');
  const separator = withoutTrackNumber.match(/\s+[-–—]\s+/);

  if (!separator || separator.index === undefined) {
    const title = clean(withoutTrackNumber);
    return title ? { title } : {};
  }

  const artist = clean(withoutTrackNumber.slice(0, separator.index));
  const title = clean(withoutTrackNumber.slice(separator.index + separator[0].length));

  return {
    ...(artist ? { artist } : {}),
    ...(title ? { title } : {}),
  };
}

function firstField(info: ArtistInfo | null | undefined, ...keys: string[]): string | undefined {
  if (!info) {
    return undefined;
  }

  for (const key of keys) {
    const values = info.fields[key];

    if (values && values.length > 0) {
      return values[0];
    }
  }

  return undefined;
}

function allFields(info: ArtistInfo | null | undefined, ...keys: string[]): string[] {
  if (!info) {
    return [];
  }

  for (const key of keys) {
    const values = info.fields[key];

    if (values && values.length > 0) {
      return values;
    }
  }

  return [];
}

/** `AIR (Salem, Montserrat)` splits into the studio and where it is. */
function splitParenthetical(value: string): {
  head: string;
  detail?: string;
} {
  const match = value.match(/^(.*?)\s*\(([^)]*)\)\s*$/);

  if (!match) {
    return { head: value };
  }

  return {
    head: clean(match[1]),
    detail: clean(match[2]),
  };
}

function joinNames(names: string[]): string | undefined {
  if (names.length === 0) {
    return undefined;
  }

  if (names.length === 1) {
    return names[0];
  }

  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/**
 * Turns the parsed sidecar into the card sequence the show cycles through. Cards whose source
 * data is missing are simply absent, so a sparse sidecar yields a short loop rather than blank
 * panels — which is why the caller must handle an empty result.
 */
export function buildNowPlayingCards(source: NowPlayingSource): NowPlayingCard[] {
  const { info } = source;
  const fromFileName = source.fileName ? parseTrackFileName(source.fileName) : {};

  // `Single by Dire Straits` / `from the album Brothers in Arms` carry the artist and album in
  // the infobox's opening lines rather than in any key.
  let leadArtist: string | undefined;
  let leadAlbum: string | undefined;
  const notes: string[] = [];

  for (const line of info?.lead ?? []) {
    const byMatch = line.match(/^(?:single|song|track|ep|album)\s+by\s+(.+)$/i);
    const albumMatch = line.match(/^from the (?:album|ep|soundtrack)\s+(.+)$/i);

    if (byMatch) {
      leadArtist = clean(byMatch[1]);
    }
    else if (albumMatch) {
      leadAlbum = clean(albumMatch[1]);
    }
    else {
      notes.push(line);
    }
  }

  const artist = firstField(info, 'artist', 'performer') ?? leadArtist ?? fromFileName.artist;
  const title = firstField(info, 'title', 'song', 'track') ?? fromFileName.title;
  const album = firstField(info, 'album') ?? leadAlbum;
  const released = firstField(info, 'released', 'release date', 'date', 'year');
  const label = firstField(info, 'label', 'labels');
  const genres = allFields(info, 'genre', 'genres');
  const bSide = firstField(info, 'b-side', 'b side');
  const writers = allFields(info, 'songwriters', 'songwriter', 'written by', 'writers', 'composer');
  const producers = allFields(info, 'producers', 'producer', 'produced by');
  const studio = firstField(info, 'studio', 'recorded at', 'recorded');
  const lengths = allFields(info, 'length', 'duration', 'runtime');

  const cards: NowPlayingCard[] = [];

  if (title) {
    cards.push({
      kicker: 'Now playing',
      headline: splitHeadline(title),
      ...(artist ? { sub: artist } : {}),
    });
  }
  else if (artist) {
    cards.push({
      kicker: 'Now playing',
      headline: splitHeadline(artist),
    });
  }

  if (album) {
    const detail = [released, label].filter(Boolean).join(' · ');
    cards.push({
      kicker: 'From the album',
      headline: splitHeadline(album),
      ...(detail ? { sub: detail } : {}),
    });
  }
  else if (released) {
    cards.push({
      kicker: 'Released',
      headline: splitHeadline(released),
    });
  }

  if (genres.length > 0) {
    cards.push({
      kicker: 'Genre',
      headline: splitHeadline(genres.join(', ')),
      ...(bSide ? { sub: `B-side — ${bSide}` } : {}),
    });
  }

  if (writers.length > 0) {
    const [lead, ...rest] = writers;
    const withOthers = joinNames(rest);
    cards.push({
      kicker: 'Written by',
      headline: splitHeadline(lead),
      ...(withOthers ? { sub: `with ${withOthers}` } : {}),
    });
  }

  if (producers.length > 0) {
    const [lead, ...rest] = producers;
    const withOthers = joinNames(rest);
    cards.push({
      kicker: 'Produced by',
      headline: splitHeadline(lead),
      ...(withOthers ? { sub: `and ${withOthers}` } : {}),
    });
  }

  if (studio) {
    const { head, detail } = splitParenthetical(studio);
    cards.push({
      kicker: 'Recorded at',
      headline: splitHeadline(head),
      ...(detail ? { sub: detail } : {}),
    });
  }

  if (lengths.length > 0) {
    const [primary, ...alternates] = lengths;
    const { head, detail } = splitParenthetical(primary);
    cards.push({
      kicker: detail ? `Length — ${detail}` : 'Length',
      headline: [head],
      ...(alternates.length > 0 ? { sub: alternates.join(' · ') } : {}),
    });
  }

  for (const note of notes) {
    cards.push({
      kicker: 'About',
      headline: artist ? splitHeadline(artist) : [],
      body: note,
    });
  }

  return cards;
}
