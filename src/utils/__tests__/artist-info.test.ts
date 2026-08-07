// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';

import {
  buildNowPlayingCards,
  parseArtistInfo,
  parseTrackFileName,
  splitHeadline,
  splitRunTogetherValues,
} from '@/utils/artist-info';

/** Verbatim copy of `~/Audios/Dire Straits/.artist/artist.info`, tabs and all. */
const WIKIPEDIA_INFOBOX = [
  'Single by Dire Straits',
  'from the album Brothers in Arms',
  'B-side\t"Love over Gold" (Live)',
  'Released\t28 June 1985[1]',
  'Studio\tAIR (Salem, Montserrat)',
  'Genre\tPop rock',
  'Length\t',
  '8:22 (album version)',
  '7:04 (LP edit)',
  '4:38 (single version)',
  '4:06 (radio edit)',
  'Label\tVertigo',
  'Songwriters\t',
  'Mark KnopflerSting',
  'Producers\t',
  'Neil DorfsmanMark Knopfler',
].join('\n');

describe('parseArtistInfo', () => {
  it('reads the lines before the first key as lead text', () => {
    const info = parseArtistInfo(WIKIPEDIA_INFOBOX);

    expect(info.lead).toEqual(['Single by Dire Straits', 'from the album Brothers in Arms']);
  });

  it('reads tab-separated keys and strips citation markers', () => {
    const info = parseArtistInfo(WIKIPEDIA_INFOBOX);

    expect(info.fields.released).toEqual(['28 June 1985']);
    expect(info.fields.genre).toEqual(['Pop rock']);
    expect(info.fields.label).toEqual(['Vertigo']);
  });

  it('collects the untabbed lines after an empty key as that key values', () => {
    const info = parseArtistInfo(WIKIPEDIA_INFOBOX);

    expect(info.fields.length).toEqual([
      '8:22 (album version)',
      '7:04 (LP edit)',
      '4:38 (single version)',
      '4:06 (radio edit)',
    ]);
  });

  it('separates list items that were pasted without separators', () => {
    const info = parseArtistInfo(WIKIPEDIA_INFOBOX);

    expect(info.fields.songwriters).toEqual(['Mark Knopfler', 'Sting']);
    expect(info.fields.producers).toEqual(['Neil Dorfsman', 'Mark Knopfler']);
  });

  it('accepts a hand-written colon form without mistaking a timestamp for a key', () => {
    const info = parseArtistInfo('Artist: Dire Straits\nLength: 8:22');

    expect(info.fields.artist).toEqual(['Dire Straits']);
    expect(info.fields.length).toEqual(['8:22']);
  });

  it('keeps a file of plain prose as lead text', () => {
    const info = parseArtistInfo('Formed in Deptford in 1977.\nKnopfler played a red Strat.');

    expect(info.fields).toEqual({});
    expect(info.lead).toHaveLength(2);
  });
});

describe('splitRunTogetherValues', () => {
  it('splits at a lowercase-to-uppercase boundary', () => {
    expect(splitRunTogetherValues('Mark KnopflerSting')).toEqual(['Mark Knopfler', 'Sting']);
  });

  it('leaves name particles intact', () => {
    expect(splitRunTogetherValues('Paul McCartney')).toEqual(['Paul McCartney']);
    expect(splitRunTogetherValues('Danny DeVito')).toEqual(['Danny DeVito']);
  });

  it('leaves a single ordinary value alone', () => {
    expect(splitRunTogetherValues('Pop rock')).toEqual(['Pop rock']);
  });
});

describe('splitHeadline', () => {
  it('breaks at the word boundary nearest the middle', () => {
    expect(splitHeadline('Money for Nothing')).toEqual(['Money for', 'Nothing']);
    expect(splitHeadline('Brothers in Arms')).toEqual(['Brothers', 'in Arms']);
  });

  it('keeps short titles on one line, however many words they hold', () => {
    expect(splitHeadline('Sultans')).toEqual(['Sultans']);
    expect(splitHeadline('So Far Away')).toEqual(['So Far Away']);
  });

  it('breaks a longer multi-word title', () => {
    expect(splitHeadline('Sultans of Swing')).toEqual(['Sultans', 'of Swing']);
  });

  it('returns nothing for empty input', () => {
    expect(splitHeadline('   ')).toEqual([]);
  });
});

describe('parseTrackFileName', () => {
  it('reads the artist and title around a dash', () => {
    expect(parseTrackFileName('Dire Straits - Money For Nothing.mp3')).toEqual({
      artist: 'Dire Straits',
      title: 'Money For Nothing',
    });
  });

  it('drops a leading track number', () => {
    expect(parseTrackFileName('03. Dire Straits - Walk of Life.flac')).toEqual({
      artist: 'Dire Straits',
      title: 'Walk of Life',
    });
  });

  it('treats a name without a separator as the title', () => {
    expect(parseTrackFileName('Telegraph Road.opus')).toEqual({ title: 'Telegraph Road' });
  });
});

describe('buildNowPlayingCards', () => {
  const cards = buildNowPlayingCards({
    info: parseArtistInfo(WIKIPEDIA_INFOBOX),
    fileName: 'Dire Straits - Money For Nothing.mp3',
  });

  it('opens with the track over the artist', () => {
    expect(cards[0]).toEqual({
      kicker: 'Now playing',
      headline: ['Money For', 'Nothing'],
      sub: 'Dire Straits',
    });
  });

  it('pairs the album with its release and label', () => {
    expect(cards[1]).toEqual({
      kicker: 'From the album',
      headline: ['Brothers', 'in Arms'],
      sub: '28 June 1985 · Vertigo',
    });
  });

  it('carries the b-side under the genre', () => {
    expect(cards[2]).toEqual({
      kicker: 'Genre',
      headline: ['Pop rock'],
      sub: 'B-side — "Love over Gold" (Live)',
    });
  });

  it('credits the first writer and producer with the rest underneath', () => {
    // Names long enough to break are stacked rather than special-cased, which is how the
    // oversized credits were set.
    expect(cards[3]).toEqual({
      kicker: 'Written by',
      headline: ['Mark', 'Knopfler'],
      sub: 'with Sting',
    });
    expect(cards[4]).toEqual({
      kicker: 'Produced by',
      headline: ['Neil', 'Dorfsman'],
      sub: 'and Mark Knopfler',
    });
  });

  it('splits the studio from where it is', () => {
    expect(cards[5]).toEqual({
      kicker: 'Recorded at',
      headline: ['AIR'],
      sub: 'Salem, Montserrat',
    });
  });

  it('promotes the parenthetical of the first length into the kicker', () => {
    expect(cards[6]).toEqual({
      kicker: 'Length — album version',
      headline: ['8:22'],
      sub: '7:04 (LP edit) · 4:38 (single version) · 4:06 (radio edit)',
    });
  });

  it('falls back to the filename when there is no sidecar at all', () => {
    const fallback = buildNowPlayingCards({
      info: null,
      fileName: 'Dire Straits - Money For Nothing.mp3',
    });

    expect(fallback).toEqual([
      {
        kicker: 'Now playing',
        headline: ['Money For', 'Nothing'],
        sub: 'Dire Straits',
      },
    ]);
  });

  it('renders leftover prose as a closing card', () => {
    const withBio = buildNowPlayingCards({
      info: parseArtistInfo('Single by Dire Straits\nFormed in Deptford in 1977.'),
    });

    expect(withBio[withBio.length - 1]).toEqual({
      kicker: 'About',
      headline: ['Dire Straits'],
      body: 'Formed in Deptford in 1977.',
    });
  });

  it('returns nothing when there is nothing to show', () => {
    expect(buildNowPlayingCards({})).toEqual([]);
  });
});
