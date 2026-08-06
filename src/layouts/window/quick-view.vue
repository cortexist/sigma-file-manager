<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { TooltipProvider } from '@/components/ui/tooltip';
</script>

<!--
  The auxiliary windows draw their own titlebar, whose buttons carry tooltips, so this layout
  needs the provider too. It keeps the provider's own default delay rather than reading the
  user setting, because these windows do not hydrate the settings store.
-->

<template>
  <TooltipProvider>
    <RouterView />
  </TooltipProvider>
</template>
