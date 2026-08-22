<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2026 Cortexist, LLC. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Share2Icon } from '@lucide/vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectItemText,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { SettingsItem } from '@/modules/settings';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { open as openSigmaDialog } from '@/utils/sigma-dialog';
import type { LanShareCertificateSource, LanShareProtocol } from '@/types/user-settings';

const userSettingsStore = useUserSettingsStore();
const { t } = useI18n();

const lanShareSettings = computed(() => userSettingsStore.userSettings.lanShare);

const protocolOptions = computed(() => [
  {
    name: t('settings.general.lanShare.protocol.httpAndHttps'),
    value: 'httpAndHttps' as LanShareProtocol,
  },
  {
    name: t('settings.general.lanShare.protocol.httpsOnly'),
    value: 'httpsOnly' as LanShareProtocol,
  },
  {
    name: t('settings.general.lanShare.protocol.httpOnly'),
    value: 'httpOnly' as LanShareProtocol,
  },
]);

const certificateSourceOptions = computed(() => [
  {
    name: t('settings.general.lanShare.certificate.selfSigned'),
    value: 'selfSigned' as LanShareCertificateSource,
  },
  {
    name: t('settings.general.lanShare.certificate.certificateFile'),
    value: 'certificateFile' as LanShareCertificateSource,
  },
]);

const selectedProtocol = computed({
  get: () => protocolOptions.value.find(
    option => option.value === lanShareSettings.value.protocol,
  ),
  set: (option) => {
    if (option) {
      userSettingsStore.set('lanShare.protocol', option.value);
    }
  },
});

const selectedCertificateSource = computed({
  get: () => certificateSourceOptions.value.find(
    option => option.value === lanShareSettings.value.certificateSource,
  ),
  set: (option) => {
    if (option) {
      userSettingsStore.set('lanShare.certificateSource', option.value);
    }
  },
});

const httpEnabled = computed(() => lanShareSettings.value.protocol !== 'httpsOnly');
const httpsEnabled = computed(() => lanShareSettings.value.protocol !== 'httpOnly');

function parsePort(value: string | number | undefined): number | null {
  const port = typeof value === 'number' ? value : parseInt(String(value ?? '').trim(), 10);
  return Number.isInteger(port) && port >= 1 && port <= 65535 ? port : null;
}

const httpPort = computed({
  get: () => lanShareSettings.value.httpPort?.toString() ?? '',
  set: value => userSettingsStore.set('lanShare.httpPort', parsePort(value)),
});

const httpsPort = computed({
  get: () => lanShareSettings.value.httpsPort?.toString() ?? '',
  set: value => userSettingsStore.set('lanShare.httpsPort', parsePort(value)),
});

const usesCertificateFile = computed(
  () => httpsEnabled.value && lanShareSettings.value.certificateSource === 'certificateFile',
);

const certificatePath = computed({
  get: () => lanShareSettings.value.certificatePath,
  set: value => userSettingsStore.set('lanShare.certificatePath', value ?? ''),
});

const privateKeyPath = computed({
  get: () => lanShareSettings.value.privateKeyPath,
  set: value => userSettingsStore.set('lanShare.privateKeyPath', value ?? ''),
});

const customHostname = computed({
  get: () => lanShareSettings.value.customHostname,
  set: value => userSettingsStore.set('lanShare.customHostname', value ?? ''),
});

async function browseForPemFile(setting: 'lanShare.certificatePath' | 'lanShare.privateKeyPath') {
  const selection = await openSigmaDialog({
    title: t('settings.general.lanShare.browseDialogTitle'),
    multiple: false,
    directory: false,
  });

  if (typeof selection === 'string' && selection) {
    await userSettingsStore.set(setting, selection);
  }
}
</script>

<template>
  <SettingsItem
    :title="t('settings.general.lanShare.title')"
    :description="t('settings.general.lanShare.description')"
    :icon="Share2Icon"
  >
    <template #nested>
      <div class="lan-share-settings">
        <div class="lan-share-settings__row">
          <div class="lan-share-settings__copy">
            <span class="lan-share-settings__label">
              {{ t('settings.general.lanShare.protocol.label') }}
            </span>
            <p class="lan-share-settings__description">
              {{ t('settings.general.lanShare.protocol.description') }}
            </p>
          </div>
          <Select
            v-model="selectedProtocol"
            by="value"
          >
            <SelectTrigger class="lan-share-settings__select-trigger">
              <SelectValue>
                {{ selectedProtocol?.name }}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="option in protocolOptions"
                :key="option.value"
                :value="option"
              >
                <SelectItemText>
                  {{ option.name }}
                </SelectItemText>
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div
          v-if="httpEnabled"
          class="lan-share-settings__row"
        >
          <div class="lan-share-settings__copy">
            <span class="lan-share-settings__label">
              {{ t('settings.general.lanShare.httpPort.label') }}
            </span>
            <p class="lan-share-settings__description">
              {{ t('settings.general.lanShare.portDescription') }}
            </p>
          </div>
          <Input
            v-model="httpPort"
            class="lan-share-settings__port-input"
            inputmode="numeric"
            :placeholder="t('settings.general.lanShare.portAutomatic')"
            spellcheck="false"
          />
        </div>

        <div
          v-if="httpsEnabled"
          class="lan-share-settings__row"
        >
          <div class="lan-share-settings__copy">
            <span class="lan-share-settings__label">
              {{ t('settings.general.lanShare.httpsPort.label') }}
            </span>
            <p class="lan-share-settings__description">
              {{ t('settings.general.lanShare.portDescription') }}
            </p>
          </div>
          <Input
            v-model="httpsPort"
            class="lan-share-settings__port-input"
            inputmode="numeric"
            :placeholder="t('settings.general.lanShare.portAutomatic')"
            spellcheck="false"
          />
        </div>

        <div
          v-if="httpsEnabled"
          class="lan-share-settings__row"
        >
          <div class="lan-share-settings__copy">
            <span class="lan-share-settings__label">
              {{ t('settings.general.lanShare.certificate.label') }}
            </span>
            <p class="lan-share-settings__description">
              {{ t('settings.general.lanShare.certificate.description') }}
            </p>
          </div>
          <Select
            v-model="selectedCertificateSource"
            by="value"
          >
            <SelectTrigger class="lan-share-settings__select-trigger">
              <SelectValue>
                {{ selectedCertificateSource?.name }}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="option in certificateSourceOptions"
                :key="option.value"
                :value="option"
              >
                <SelectItemText>
                  {{ option.name }}
                </SelectItemText>
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div
          v-if="usesCertificateFile"
          class="lan-share-settings__field"
        >
          <span class="lan-share-settings__label">
            {{ t('settings.general.lanShare.certificatePath.label') }}
          </span>
          <div class="lan-share-settings__path-row">
            <Input
              v-model="certificatePath"
              :placeholder="t('settings.general.lanShare.certificatePath.placeholder')"
              spellcheck="false"
            />
            <Button
              variant="secondary"
              size="sm"
              @click="browseForPemFile('lanShare.certificatePath')"
            >
              {{ t('settings.general.lanShare.browse') }}
            </Button>
          </div>
        </div>

        <div
          v-if="usesCertificateFile"
          class="lan-share-settings__field"
        >
          <span class="lan-share-settings__label">
            {{ t('settings.general.lanShare.privateKeyPath.label') }}
          </span>
          <div class="lan-share-settings__path-row">
            <Input
              v-model="privateKeyPath"
              :placeholder="t('settings.general.lanShare.privateKeyPath.placeholder')"
              spellcheck="false"
            />
            <Button
              variant="secondary"
              size="sm"
              @click="browseForPemFile('lanShare.privateKeyPath')"
            >
              {{ t('settings.general.lanShare.browse') }}
            </Button>
          </div>
        </div>

        <div class="lan-share-settings__field">
          <span class="lan-share-settings__label">
            {{ t('settings.general.lanShare.hostname.label') }}
          </span>
          <p class="lan-share-settings__description">
            {{ t('settings.general.lanShare.hostname.description') }}
          </p>
          <Input
            v-model="customHostname"
            class="lan-share-settings__hostname-input"
            :placeholder="t('settings.general.lanShare.hostname.placeholder')"
            spellcheck="false"
          />
        </div>
      </div>
    </template>
  </SettingsItem>
</template>

<style scoped>
.lan-share-settings {
  display: flex;
  width: 100%;
  flex-direction: column;
  gap: 1rem;
}

.lan-share-settings__row {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem 2rem;
}

.lan-share-settings__copy {
  display: flex;
  min-width: min(100%, 16rem);
  flex: 1 1 12rem;
  flex-direction: column;
}

.lan-share-settings__label {
  color: hsl(var(--foreground));
  font-size: 0.875rem;
}

.lan-share-settings__description {
  margin: 0;
  color: hsl(var(--muted-foreground));
  font-size: 0.875rem;
  line-height: 1.4;
}

.lan-share-settings__select-trigger {
  width: min(100%, 14rem);
}

.lan-share-settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.lan-share-settings__path-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.lan-share-settings__hostname-input {
  width: min(100%, 18rem);
}

.lan-share-settings__port-input {
  width: min(100%, 8rem);
}
</style>
