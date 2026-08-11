<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<!--
  A trash can with a magnifier in it: looking through what was deleted, not deleting.

  The rail's other icons are Lucide outlines, and a plain trash can among them would read as the
  delete action rather than a place to go. This is drawn as filled paths rather than strokes, but
  at the same relative weight Lucide uses (its 2px stroke on a 24 viewBox is the same fraction of
  the frame as the 42 unit bars here are of 512), so it sits at the same visual weight as its
  neighbours.
-->

<script setup lang="ts">
withDefaults(defineProps<{
  size?: number | string;
}>(), {
  size: 24,
});
</script>

<template>
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 512 512"
    fill="none"
    aria-hidden="true"
    :width="size"
    :height="size"
  >
    <path
      d="M415.177,462.484c-11.517,16.766 -30.821,27.85 -52.51,27.85l-213.333,0c-34.927,0 -63.667,-28.74 -63.667,-63.667l0,-277.667l-21.667,-0c-11.59,-0 -21,-9.41 -21,-21c0,-11.59 9.41,-21 21,-21l85.667,0l0,-21.667c0,-34.927 28.74,-63.667 63.667,-63.667l85.333,0c34.927,-0 63.667,28.74 63.667,63.667l0,21.667l85.667,0c11.59,-0 21,9.41 21,21c0,11.59 -9.41,21 -21,21l-21.667,0l-0,134.002c13.112,18.747 20.807,41.555 20.807,66.147c0,24.321 -7.527,46.898 -20.376,65.526l37.876,37.876c8.196,8.196 8.196,21.503 0,29.698c-8.196,8.196 -21.503,8.196 -29.698,0l-19.765,-19.765Zm-94.843,-355.484l0,-21.667c0,-11.886 -9.781,-21.667 -21.667,-21.667l-85.333,0c-11.886,0 -21.667,9.781 -21.667,21.667l0,21.667l128.667,0Zm64,139.297l0,-97.297l-256.667,0l-0,277.667c0,11.886 9.781,21.667 21.667,21.667l122.829,0c-24.455,-14.681 -43.044,-38.15 -51.388,-66.025c-2.313,0.878 -4.821,1.358 -7.44,1.358c-11.59,0 -21,-9.41 -21,-21l0,-128c0,-11.59 9.41,-21 21,-21c11.59,0 21,9.41 21,21l0,51.926c10.763,-16.681 25.702,-30.42 43.333,-39.733l0,-12.194c-0,-11.59 9.41,-21 21,-21c11.418,-0 20.72,9.132 20.994,20.485c3.906,-0.399 7.869,-0.604 11.88,-0.604c19.015,0 36.964,4.601 52.793,12.749Zm-52.793,29.251c-40.621,0 -73.601,32.979 -73.601,73.601c0,40.621 32.979,73.601 73.601,73.601c40.621,0 73.601,-32.979 73.601,-73.601c0,-40.621 -32.979,-73.601 -73.601,-73.601Z"
      fill="currentColor"
    />
  </svg>
</template>
