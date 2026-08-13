// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import type { SigmaExtensionAPI } from '@/modules/extensions/api';

export type ExtensionSandbox = {
  extensionId: string;
  globalContext: Record<string, unknown>;
  console: Console;
  evaluate: (code: string) => Record<string, unknown>;
  destroy: () => void;
};

const BLOCKED_GLOBALS = [
  'eval',
  'Function',
  'fetch',
  'XMLHttpRequest',
  'WebSocket',
  'localStorage',
  'sessionStorage',
  'indexedDB',
  'document',
  'window',
  'globalThis',
  'parent',
  'top',
  'frames',
  'opener',
  'self',
];

const ALLOWED_GLOBALS = [
  'console',
  'Math',
  'Date',
  'JSON',
  'Array',
  'Object',
  'String',
  'Number',
  'Boolean',
  'Symbol',
  'Map',
  'Set',
  'WeakMap',
  'WeakSet',
  'Promise',
  'Proxy',
  'Reflect',
  'Error',
  'TypeError',
  'RangeError',
  'SyntaxError',
  'ReferenceError',
  'URIError',
  'EvalError',
  'parseInt',
  'parseFloat',
  'isNaN',
  'isFinite',
  'encodeURI',
  'decodeURI',
  'encodeURIComponent',
  'decodeURIComponent',
  'setTimeout',
  'clearTimeout',
  'setInterval',
  'clearInterval',
  'queueMicrotask',
  'atob',
  'btoa',
  'TextEncoder',
  'TextDecoder',
  'URL',
  'URLSearchParams',
  'Blob',
  'File',
  'FileReader',
  'AbortController',
  'AbortSignal',
  'crypto',
  'performance',
  'structuredClone',
];

const HOST_BOUND_FUNCTIONS = new Set([
  'setTimeout',
  'clearTimeout',
  'setInterval',
  'clearInterval',
  'queueMicrotask',
  'atob',
  'btoa',
  'structuredClone',
]);

export function createSandbox(
  extensionId: string,
  api: SigmaExtensionAPI,
): ExtensionSandbox {
  const globalContext: Record<string, unknown> = {};

  for (const name of ALLOWED_GLOBALS) {
    const globalValue = (globalThis as Record<string, unknown>)[name];

    if (globalValue !== undefined) {
      if (HOST_BOUND_FUNCTIONS.has(name) && typeof globalValue === 'function') {
        globalContext[name] = (globalValue as (...args: unknown[]) => unknown).bind(globalThis);
      }
      else {
        globalContext[name] = globalValue;
      }
    }
  }

  globalContext.sigma = Object.freeze(api);

  for (const name of BLOCKED_GLOBALS) {
    globalContext[name] = undefined;
  }

  const proxyHandler: ProxyHandler<Record<string, unknown>> = {
    get(target, prop: string) {
      if (BLOCKED_GLOBALS.includes(prop)) {
        console.warn(`Extension ${extensionId} attempted to access blocked global: ${prop}`);
        return undefined;
      }

      if (prop === 'sigma') {
        return target.sigma;
      }

      return target[prop];
    },
    set(target, prop: string, value) {
      if (prop === 'sigma' || BLOCKED_GLOBALS.includes(prop)) {
        console.warn(`Extension ${extensionId} attempted to modify protected property: ${prop}`);
        return false;
      }

      target[prop] = value;
      return true;
    },
    has(target, prop: string) {
      if (BLOCKED_GLOBALS.includes(prop)) {
        return true;
      }

      return prop in target;
    },
  };

  const sandboxedContext = new Proxy(globalContext, proxyHandler);

  const sandboxedConsole = {
    log: (...args: unknown[]) => console.log(`[Extension ${extensionId}]`, ...args),
    warn: (...args: unknown[]) => console.warn(`[Extension ${extensionId}]`, ...args),
    error: (...args: unknown[]) => console.error(`[Extension ${extensionId}]`, ...args),
    info: (...args: unknown[]) => console.info(`[Extension ${extensionId}]`, ...args),
    debug: (...args: unknown[]) => console.debug(`[Extension ${extensionId}]`, ...args),
  } as Console;
  globalContext.console = sandboxedConsole;

  function evaluate(code: string): Record<string, unknown> {
    const executor = new Function(
      'globalContext',
      `
      with (globalContext) {
        const module = { exports: {} };
        const exports = module.exports;
        ${code}
        return module.exports;
      }
      `,
    );
    const result = executor(sandboxedContext);
    return (result && typeof result === 'object') ? result as Record<string, unknown> : {};
  }

  return {
    extensionId,
    globalContext: sandboxedContext,
    console: sandboxedConsole,
    evaluate,
    destroy: () => {
      for (const key of Object.keys(globalContext)) {
        delete globalContext[key];
      }
    },
  };
}

export function createSandboxedFunction(): () => Promise<void> {
  return async () => {
    console.warn('Sandboxed function execution is limited in this version');
  };
}

/**
 * Characters after which a `/` begins a regular expression rather than a division. The
 * scanner needs to tell the two apart, because a character class such as `/['"]/` would
 * otherwise look like the start of a string and swallow the rest of the file.
 */
const REGEX_ALLOWED_AFTER = new Set('(,=:[!&|?{};+-*%~^<>'.split(''));

/**
 * Blanks out comments, leaving everything else — including string contents — in place.
 *
 * The checks below are text searches, and prose is text. A doc comment that merely
 * mentions the navigator, or a window, or the document, is not an access to any of them,
 * but it reads identically to one. Those words are ordinary vocabulary in a file manager,
 * so scanning comments rejects reasonable extensions with a security error that points at
 * nothing. Strings are deliberately left alone: a literal `eval(` in a string is still
 * worth flagging, and blanking strings would weaken the check for no clear gain.
 *
 * Comments are replaced by spaces rather than removed so that every remaining character
 * keeps its offset, and newlines are preserved so line-anchored patterns still behave.
 */
export function stripComments(code: string): string {
  const output = code.split('');
  let index = 0;
  let lastSignificantCharacter = '';

  function blank(from: number, to: number): void {
    for (let position = from; position < to; position += 1) {
      if (output[position] !== '\n') {
        output[position] = ' ';
      }
    }
  }

  while (index < code.length) {
    const character = code[index];
    const nextCharacter = code[index + 1];

    if (character === '/' && nextCharacter === '/') {
      const lineEnd = code.indexOf('\n', index);
      const end = lineEnd === -1 ? code.length : lineEnd;
      blank(index, end);
      index = end;
      continue;
    }

    if (character === '/' && nextCharacter === '*') {
      const commentEnd = code.indexOf('*/', index + 2);
      const end = commentEnd === -1 ? code.length : commentEnd + 2;
      blank(index, end);
      index = end;
      continue;
    }

    if (character === '\'' || character === '"' || character === '`') {
      index = skipQuoted(code, index, character);
      lastSignificantCharacter = character;
      continue;
    }

    if (character === '/' && REGEX_ALLOWED_AFTER.has(lastSignificantCharacter)) {
      index = skipRegex(code, index);
      lastSignificantCharacter = '/';
      continue;
    }

    if (!/\s/.test(character)) {
      lastSignificantCharacter = character;
    }

    index += 1;
  }

  return output.join('');
}

/** Returns the offset just past the closing quote, honouring backslash escapes. */
function skipQuoted(code: string, start: number, quote: string): number {
  let index = start + 1;

  while (index < code.length) {
    const character = code[index];

    if (character === '\\') {
      index += 2;
      continue;
    }

    if (character === quote) {
      return index + 1;
    }

    // An unterminated single-quoted string is a syntax error, but the scanner should not
    // run past the end of the line looking for a quote that is never coming.
    if (character === '\n' && quote !== '`') {
      return index;
    }

    index += 1;
  }

  return index;
}

/** Returns the offset just past a regular expression literal, including its flags. */
function skipRegex(code: string, start: number): number {
  let index = start + 1;
  let inCharacterClass = false;

  while (index < code.length) {
    const character = code[index];

    if (character === '\\') {
      index += 2;
      continue;
    }

    if (character === '[') {
      inCharacterClass = true;
    }
    else if (character === ']') {
      inCharacterClass = false;
    }
    else if (character === '/' && !inCharacterClass) {
      return index + 1;
    }
    else if (character === '\n') {
      return index;
    }

    index += 1;
  }

  return index;
}

export function validateExtensionCode(code: string): {
  valid: boolean;
  errors: string[];
} {
  const errors: string[] = [];

  const dangerousPatterns = [
    {
      pattern: /\beval\s*\(/g,
      message: 'eval() is not allowed',
    },
    {
      pattern: /\bnew\s+Function\s*\(/g,
      message: 'new Function() is not allowed',
    },
    {
      pattern: /\.constructor\s*\(\s*['"`]/g,
      message: 'Dynamic code construction via .constructor() is not allowed',
    },
    {
      pattern: /\bdocument\s*\./g,
      message: 'Direct document access is not allowed',
    },
    {
      pattern: /\bwindow\s*\./g,
      message: 'Direct window access is not allowed',
    },
    {
      pattern: /\blocalStorage\s*\./g,
      message: 'localStorage is not allowed',
    },
    {
      pattern: /\bsessionStorage\s*\./g,
      message: 'sessionStorage is not allowed',
    },
    {
      pattern: /\bindexedDB\s*\./g,
      message: 'indexedDB is not allowed',
    },
    {
      pattern: /\bfetch\s*\(/g,
      message: 'fetch() is not allowed - use sigma.fs instead',
    },
    {
      pattern: /\bXMLHttpRequest\b/g,
      message: 'XMLHttpRequest is not allowed',
    },
    {
      pattern: /\bWebSocket\b/g,
      message: 'WebSocket is not allowed',
    },
    {
      pattern: /(?<![\w$.])self\b/g,
      message: 'Direct self access is not allowed',
    },
    {
      pattern: /(?<![\w$.])globalThis\b/g,
      message: 'Direct globalThis access is not allowed',
    },
    {
      pattern: /(?<![\w$.])postMessage\s*\(/g,
      message: 'Direct postMessage() is not allowed',
    },
    {
      pattern: /(?<![\w$.])addEventListener\s*\(/g,
      message: 'Direct addEventListener() is not allowed',
    },
    {
      pattern: /(?<![\w$.])removeEventListener\s*\(/g,
      message: 'Direct removeEventListener() is not allowed',
    },
    {
      pattern: /(?<![\w$.])dispatchEvent\s*\(/g,
      message: 'Direct dispatchEvent() is not allowed',
    },
    {
      pattern: /\bimportScripts\b/g,
      message: 'importScripts is not allowed',
    },
    {
      pattern: /(?<![\w$.])navigator\b/g,
      message: 'Direct navigator access is not allowed',
    },
  ];

  const executableCode = stripComments(code);

  for (const { pattern, message } of dangerousPatterns) {
    if (pattern.test(executableCode)) {
      errors.push(message);
    }
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}

export function freezeObject<T extends object>(obj: T): T {
  Object.freeze(obj);

  for (const value of Object.values(obj)) {
    if (value && typeof value === 'object' && !Object.isFrozen(value)) {
      freezeObject(value as object);
    }
  }

  return obj;
}
