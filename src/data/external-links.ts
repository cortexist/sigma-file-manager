// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

/*
  This fork, not the upstream project it came from.

  Everything here is where a person using *this* build should be sent: the releases they can
  actually install, the issue tracker whose maintainer can act on their report, and the source
  that matches the binary in front of them. Upstream keeps the credit in the licence headers and
  the README; it should not be receiving bug reports about code it has never seen.
*/
const githubMainBranch = 'main';
const githubUser = 'cortexist';
const githubRepo = 'sigma-file-manager';
const githubFullRepoName = `${githubUser}/${githubRepo}`;
const githubBaseUrl = 'https://github.com';
const githubBaseApiUrl = 'https://api.github.com/repos';
const githubRawApiUrl = 'https://raw.githubusercontent.com';
const githubUserLink = `${githubBaseUrl}/${githubUser}`;
const githubRepoLink = `${githubBaseUrl}/${githubFullRepoName}`;
const githubRepoApiLink = `${githubBaseApiUrl}/${githubFullRepoName}`;
const githubLicenseLink = `${githubRepoLink}/blob/${githubMainBranch}/LICENSE.md`;
const githubChangelogLink = `${githubRepoLink}/blob/${githubMainBranch}/CHANGELOG.md`;
const githubIssuesLink = `${githubRepoLink}/issues/`;
const githubDiscussionsLink = `${githubRepoLink}/discussions/`;
const githubIssueTemplateProblemReport = `${githubRepoLink}/issues/new?template=Problem_report.md`;
const githubIssueTemplateFeatureRequest = `${githubRepoLink}/issues/new?template=Feature_request.md`;
const githubAllReleases = `${githubRepoLink}/releases`;
const githubLatestRelease = `${githubRepoLink}/releases/latest`;
const githubReadmeSupportSectionLink = `${githubRepoLink}#supporters`;

const extensionsRegistryRepo = 'sfm-marketplace';
const extensionsRegistryOrg = 'sigma-hub';
const extensionsRegistryFullRepoName = `${extensionsRegistryOrg}/${extensionsRegistryRepo}`;
const extensionsRegistryBaseUrl = `${githubRawApiUrl}/${extensionsRegistryFullRepoName}/main`;
const extensionsRegistryUrl = `${extensionsRegistryBaseUrl}/registry.json`;
const extensionsSubmissionGuideUrl = `${githubBaseUrl}/${extensionsRegistryFullRepoName}#readme`;
const extensionsDevelopmentWikiUrl = `${githubBaseUrl}/${extensionsRegistryFullRepoName}/wiki`;
const extensionsApiBaseUrl = `${githubRawApiUrl}/${githubFullRepoName}/${githubMainBranch}/packages/api`;
const extensionsTypesUrl = `${extensionsApiBaseUrl}/index.d.ts`;
const extensionsManifestSchemaUrl = `${extensionsApiBaseUrl}/manifest.schema.json`;

export default {
  githubRepo,
  githubUserLink,
  githubRepoLink,
  githubRepoApiLink,
  githubLicenseLink,
  githubChangelogLink,
  githubIssuesLink,
  githubDiscussionsLink,
  githubIssueTemplateProblemReport,
  githubIssueTemplateFeatureRequest,
  githubAllReleases,
  githubLatestRelease,
  githubReadmeSupportSectionLink,
  extensionsRegistryUrl,
  extensionsSubmissionGuideUrl,
  extensionsDevelopmentWikiUrl,
  extensionsTypesUrl,
  extensionsManifestSchemaUrl,
};
