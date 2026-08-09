// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import {
  beforeEach, describe, expect, it, vi,
} from 'vitest';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
  convertFileSrc: (path: string) => `asset://${path}`,
}));

import { clearArtistShowCache, loadArtistShow } from '@/composables/use-artist-show';

const ARTIST_INFO = [
  'Single by Dire Straits',
  'from the album Brothers in Arms',
  'Genre\tPop rock',
  'Label\tVertigo',
].join('\n');

function entry(path: string, name: string, isFile: boolean) {
  return {
    path,
    name,
    is_file: isFile,
  };
}

/**
 * Stands in for the Rust side: a directory tree keyed by path, plus the bytes of any file the
 * sidecar reader asks for.
 */
function mockFilesystem(tree: Record<string, ReturnType<typeof entry>[]>, files: Record<string, string> = {}) {
  invokeMock.mockImplementation((command: string, args: Record<string, unknown>) => {
    if (command === 'read_dir') {
      const entries = tree[args.path as string];

      if (!entries) {
        return Promise.reject(new Error('No such directory'));
      }

      return Promise.resolve({ entries });
    }

    if (command === 'read_text_preview') {
      const text = files[args.path as string];

      if (text === undefined) {
        return Promise.reject(new Error('No such file'));
      }

      return Promise.resolve({
        bytes: Array.from(new TextEncoder().encode(text)),
        truncated: false,
      });
    }

    return Promise.reject(new Error(`Unexpected command ${command}`));
  });
}

describe('loadArtistShow', () => {
  beforeEach(() => {
    clearArtistShowCache();
    invokeMock.mockReset();
  });

  it('reads photos and the sidecar from a hidden folder beside the track', async () => {
    mockFilesystem(
      {
        '/music/Dire Straits': [
          entry('/music/Dire Straits/.artist', '.artist', false),
          entry('/music/Dire Straits/track.mp3', 'track.mp3', true),
        ],
        '/music/Dire Straits/.artist': [
          entry('/music/Dire Straits/.artist/b.jpg', 'b.jpg', true),
          entry('/music/Dire Straits/.artist/a.jpg', 'a.jpg', true),
          entry('/music/Dire Straits/.artist/artist.info', 'artist.info', true),
        ],
      },
      { '/music/Dire Straits/.artist/artist.info': ARTIST_INFO },
    );

    const show = await loadArtistShow('/music/Dire Straits/Dire Straits - Money For Nothing.mp3');

    // Sorted by name, so the sequence is the same on every run rather than filesystem order.
    expect(show.photos).toEqual([
      'asset:///music/Dire Straits/.artist/a.jpg',
      'asset:///music/Dire Straits/.artist/b.jpg',
    ]);
    expect(show.cards[0]).toEqual({
      kicker: 'Now playing',
      headline: ['Money For', 'Nothing'],
      sub: 'Dire Straits',
    });
    expect(show.cards.map(card => card.kicker)).toContain('From the album');
  });

  it('looks one level up so an artist folder can serve every album under it', async () => {
    mockFilesystem(
      {
        '/music/Dire Straits/Brothers in Arms': [
          entry('/music/Dire Straits/Brothers in Arms/track.mp3', 'track.mp3', true),
        ],
        '/music/Dire Straits': [
          entry('/music/Dire Straits/artist', 'artist', false),
          entry('/music/Dire Straits/Brothers in Arms', 'Brothers in Arms', false),
        ],
        '/music/Dire Straits/artist': [
          entry('/music/Dire Straits/artist/press.png', 'press.png', true),
        ],
      },
    );

    const show = await loadArtistShow('/music/Dire Straits/Brothers in Arms/01 - So Far Away.mp3');

    expect(show.photos).toEqual(['asset:///music/Dire Straits/artist/press.png']);
    expect(show.cards[0]).toEqual({
      kicker: 'Now playing',
      headline: ['So Far Away'],
      sub: undefined,
    });
  });

  it('still builds cards from the file name when no folder exists', async () => {
    mockFilesystem({ '/music/loose': [entry('/music/loose/track.mp3', 'track.mp3', true)] });

    const show = await loadArtistShow('/music/loose/Dire Straits - Walk of Life.mp3');

    expect(show.photos).toEqual([]);
    expect(show.cards).toEqual([
      {
        kicker: 'Now playing',
        headline: ['Walk of Life'],
        sub: 'Dire Straits',
      },
    ]);
  });

  it('ignores files that are not images', async () => {
    mockFilesystem({
      '/music/x': [entry('/music/x/.artist', '.artist', false)],
      '/music/x/.artist': [
        entry('/music/x/.artist/notes.md', 'notes.md', true),
        entry('/music/x/.artist/photo.webp', 'photo.webp', true),
      ],
    });

    const show = await loadArtistShow('/music/x/A - B.mp3');

    expect(show.photos).toEqual(['asset:///music/x/.artist/photo.webp']);
  });

  it('resolves a directory once and serves every later track from the cache', async () => {
    mockFilesystem({
      '/music/y': [entry('/music/y/.artist', '.artist', false)],
      '/music/y/.artist': [entry('/music/y/.artist/p.jpg', 'p.jpg', true)],
    });

    await loadArtistShow('/music/y/A - One.mp3');
    const callsAfterFirst = invokeMock.mock.calls.length;
    await loadArtistShow('/music/y/A - Two.mp3');

    expect(invokeMock.mock.calls.length).toBe(callsAfterFirst);
  });
});
