// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

import type { UIElement, UISelectOption } from '@/types/extension';

/**
 * The `sigma.ui.*` element builders, in one place.
 *
 * An extension reaches these twice over: from a worker, which needs them locally so that
 * describing a form is not a round trip per element, and from the host for extensions
 * that run in the page. Keeping a copy on each side meant a field added to one was
 * silently dropped by the other — a button's icon and tooltip went missing that way, and
 * the button simply rendered blank with nothing to say why.
 */

export interface ButtonElementOptions {
  id: string;
  /** May be omitted when an icon is given, producing an icon-only button. */
  label?: string;
  icon?: string;
  tooltip?: string;
  loading?: boolean;
  variant?: 'primary' | 'secondary' | 'danger';
  size?: 'xs' | 'sm' | 'default' | 'lg';
  disabled?: boolean;
}

export interface ImageElementOptions {
  id?: string;
  /** An empty source renders a placeholder frame, so a layout keeps its shape. */
  src: string;
  alt?: string;
}

export interface SkeletonElementOptions {
  id?: string;
  width?: number;
  height?: number;
}

export interface PreviewCardElementOptions {
  thumbnail: string;
  title: string;
  subtitle?: string;
}

export interface AlertElementOptions {
  title: string;
  description?: string;
  tone?: 'info' | 'success' | 'warning' | 'error';
}

export interface SelectElementOptions {
  id: string;
  label?: string;
  placeholder?: string;
  options: UISelectOption[];
  value?: string;
  disabled?: boolean;
}

export function buildButtonElement(options: ButtonElementOptions): UIElement {
  return {
    type: 'button',
    id: options.id,
    label: options.label ?? '',
    icon: options.icon,
    tooltip: options.tooltip,
    loading: options.loading,
    variant: options.variant,
    size: options.size ?? 'xs',
    disabled: options.disabled,
  };
}

export function buildImageElement(options: ImageElementOptions): UIElement {
  return {
    type: 'image',
    id: options.id,
    value: options.src,
    label: options.alt,
  };
}

export function buildPreviewCardElement(options: PreviewCardElementOptions): UIElement {
  return {
    type: 'previewCard',
    value: options.thumbnail,
    label: options.title,
    subtitle: options.subtitle ?? '',
  };
}

export function buildSkeletonElement(options?: SkeletonElementOptions): UIElement {
  const hasDimensions = options?.width !== undefined && options?.height !== undefined;

  return {
    type: 'skeleton',
    id: options?.id,
    value: hasDimensions ? `${options?.width}x${options?.height}` : undefined,
  };
}

export function buildAlertElement(options: AlertElementOptions): UIElement {
  return {
    type: 'alert',
    label: options.title,
    value: options.description ?? '',
    tone: options.tone ?? 'info',
  };
}

export function buildSelectElement(options: SelectElementOptions): UIElement {
  return {
    type: 'select',
    id: options.id,
    label: options.label,
    placeholder: options.placeholder,
    options: options.options,
    value: options.value,
    disabled: options.disabled,
  };
}
