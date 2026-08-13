// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { describe, expect, it } from 'vitest';
import {
  getManifestPermissionHosts,
  getManifestPermissionKey,
  isValidHttpHostPattern,
  parseExtensionPermissions,
} from '@/modules/extensions/utils/parse-extension-permissions';

describe('parseExtensionPermissions', () => {
  it('rejects plain http permission without host allowlist', () => {
    expect(() => parseExtensionPermissions(['commands', 'http'])).toThrow(
      'Invalid manifest: http permission requires a host allowlist object',
    );
  });

  it('parses view permission', () => {
    expect(parseExtensionPermissions(['commands', 'view'])).toEqual({
      permissions: ['commands', 'view'],
    });
  });

  it('parses http permission with host allowlist', () => {
    expect(parseExtensionPermissions([
      {
        name: 'http',
        hosts: ['http://localhost:*', 'http://127.0.0.1:8080'],
      },
    ])).toEqual({
      permissions: ['http'],
      httpAllowedHosts: ['http://localhost:*', 'http://127.0.0.1:8080'],
    });
  });

  it('rejects duplicate http permissions', () => {
    expect(() => parseExtensionPermissions([
      {
        name: 'http',
        hosts: ['http://localhost:*'],
      },
      {
        name: 'http',
        hosts: ['https://example.com'],
      },
    ])).toThrow('duplicate http permission');
  });

  it('validates host patterns', () => {
    expect(isValidHttpHostPattern('http://localhost:*')).toBe(true);
    expect(isValidHttpHostPattern('https://api.example.com')).toBe(true);
    expect(isValidHttpHostPattern('ftp://localhost:*')).toBe(false);
  });

  it('extracts permission metadata for UI', () => {
    const hostPermission = {
      name: 'http' as const,
      hosts: ['http://localhost:*'],
    };

    expect(getManifestPermissionKey(hostPermission)).toBe('http');
    expect(getManifestPermissionHosts(hostPermission)).toEqual(['http://localhost:*']);
  });
});

describe('subdomain wildcard host patterns', () => {
  /**
   * The case this exists for: the Cover Art Archive redirects to a per-request archive.org
   * CDN hostname, which an extension cannot name in advance. The host check runs against
   * the final URL after redirects, so without a wildcard the request is refused.
   */
  it('accepts a leading wildcard label', () => {
    expect(isValidHttpHostPattern('https://*.archive.org')).toBe(true);
    expect(isValidHttpHostPattern('https://*.archive.org:*')).toBe(true);
  });

  it('rejects a wildcard anywhere else', () => {
    expect(isValidHttpHostPattern('https://ia*.archive.org')).toBe(false);
    expect(isValidHttpHostPattern('https://archive.*')).toBe(false);
  });

  it('rejects a wildcard with no domain behind it', () => {
    expect(isValidHttpHostPattern('https://*.')).toBe(false);
  });

  it('leaves exact patterns as they were', () => {
    expect(isValidHttpHostPattern('https://musicbrainz.org')).toBe(true);
    expect(isValidHttpHostPattern('ftp://example.com')).toBe(false);
  });
});
