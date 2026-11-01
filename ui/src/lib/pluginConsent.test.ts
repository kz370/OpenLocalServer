import test from 'node:test'
import assert from 'node:assert/strict'
import type { PluginManifest } from '../core/types/system.ts'
import {
  buildPluginConsentSummary,
  extractHostname,
  formatDownloadSource,
  formatSize,
} from './pluginConsent.ts'

test('extractHostname correctly parses hostnames from runtime URLs', () => {
  assert.equal(
    extractHostname('https://go.dev/dl/go1.27.1.windows-amd64.zip'),
    'go.dev'
  )
  assert.equal(
    extractHostname('https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-windows-x64.zip'),
    'github.com'
  )
  assert.equal(
    extractHostname('https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.401/dotnet-sdk-10.0.401-win-x64.zip'),
    'builds.dotnet.microsoft.com'
  )
  assert.equal(extractHostname(''), '')
  assert.equal(extractHostname('invalid-url'), '')
})

test('formatDownloadSource formats github.com and non-github URLs with ports and sanitization', () => {
  // github.com: host/owner/repo
  assert.equal(
    formatDownloadSource('https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-windows-x64.zip'),
    'github.com/oven-sh/bun'
  )
  assert.equal(
    formatDownloadSource('https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12%2B1/OpenJDK21U.zip'),
    'github.com/adoptium/temurin21-binaries'
  )

  // non-github: host + first path segment
  assert.equal(
    formatDownloadSource('https://go.dev/dl/go1.27.1.windows-amd64.zip'),
    'go.dev/dl'
  )
  assert.equal(
    formatDownloadSource('https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.401/dotnet-sdk.zip'),
    'builds.dotnet.microsoft.com/dotnet'
  )

  // non-github with empty path
  assert.equal(
    formatDownloadSource('https://mirror.example.org/'),
    'mirror.example.org'
  )
  assert.equal(
    formatDownloadSource('https://mirror.example.org'),
    'mirror.example.org'
  )

  // Strips query strings, hashes, and credentials
  assert.equal(
    formatDownloadSource('https://user:secret123@download.example.com/packages/tool.zip?token=xyz&auth=1#v1'),
    'download.example.com/packages'
  )

  // Keeps non-default ports, strips default ports
  assert.equal(
    formatDownloadSource('https://custom.mirror.org:8443/runtimes/download.zip'),
    'custom.mirror.org:8443/runtimes'
  )
  assert.equal(
    formatDownloadSource('https://custom.mirror.org:443/runtimes/download.zip'),
    'custom.mirror.org/runtimes'
  )
  assert.equal(
    formatDownloadSource('http://internal.lan:8080/dist/tool.zip'),
    'internal.lan:8080/dist'
  )
  assert.equal(
    formatDownloadSource('http://internal.lan:80/dist/tool.zip'),
    'internal.lan/dist'
  )
})

test('formatSize formats numbers and strings, and returns null when omitted', () => {
  assert.equal(formatSize(undefined), null)
  assert.equal(formatSize(null), null)
  assert.equal(formatSize(''), null)

  // String input
  assert.equal(formatSize('45 MB'), 'about 45 MB')
  assert.equal(formatSize('about 205 MB'), 'about 205 MB')

  // Numeric byte inputs
  assert.equal(formatSize(47185920), 'about 45 MB')
  assert.equal(formatSize(512000), 'about 500 KB')
  assert.equal(formatSize(1610612736), 'about 1.5 GB')
})

test('buildPluginConsentSummary formats Bun manifest with full source', () => {
  const bunManifest: PluginManifest = {
    id: 'bun',
    name: 'Bun',
    version: '1.0.0',
    description: 'The Bun JavaScript runtime and package manager. Installs under Runtimes.',
    author: 'OpenLocalServer',
    homepage: '',
    kind: 'declarative',
    permissions: ['download', 'read_projects'],
    contributes: {
      runtimes: [
        {
          id: 'bun',
          name: 'Bun',
          version: '1.4.2',
          url: 'https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-windows-x64.zip',
          sha256: 'ce4c17497b2f29712a99d3d53f028de28cd42e3bacb8589599e7f000e49b6405',
        },
      ],
      quick_apps: null,
      detections: [
        {
          id: 'bun-project',
          name: 'Bun',
          markers: ['bun.lockb', 'bun.lock', 'bunfig.toml'],
        },
      ],
      health_checks: [],
    },
  }

  const summary = buildPluginConsentSummary(bunManifest)

  // Runtimes
  assert.equal(summary.runtimes.length, 1)
  assert.equal(summary.runtimes[0].name, 'Bun')
  assert.equal(summary.runtimes[0].version, '1.4.2')
  assert.equal(summary.runtimes[0].host, 'github.com')
  assert.equal(summary.runtimes[0].source, 'github.com/oven-sh/bun')
  assert.equal(summary.runtimes[0].sizeText, null) // no size in manifest
  assert.match(summary.runtimes[0].explanation, /Only downloads when you click Install on the Runtimes page/)

  // Footer closing sentence does NOT duplicate "nothing downloads"
  assert.doesNotMatch(summary.closingSentence, /nothing downloads/i)
  assert.equal(
    summary.closingSentence,
    'Once turned on, its runtimes appear on the Runtimes page. You can turn this plugin off at any time from the Plugins page.'
  )

  // Detections
  assert.ok(summary.detections)
  assert.equal(summary.detections.items.length, 1)
  assert.deepEqual(summary.detections.items[0].markers, ['bun.lockb', 'bun.lock', 'bunfig.toml'])
})

test('buildPluginConsentSummary formats Java manifest with multiple detections and full source', () => {
  const javaManifest: PluginManifest = {
    id: 'java',
    name: 'Java (Temurin JDK)',
    version: '1.0.0',
    description: 'Eclipse Temurin JDK 21 (long-term support), from Adoptium.',
    author: 'OpenLocalServer',
    homepage: '',
    kind: 'declarative',
    permissions: ['download', 'read_projects'],
    contributes: {
      runtimes: [
        {
          id: 'java',
          name: 'Java (Temurin JDK)',
          version: '21.0.12.1',
          url: 'https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12%2B1/OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip',
          sha256: 'f9d6e191ab098c0d416e7d588a24420a8621cd2f4720dab2459b8b7b2d2d8b4e',
        },
      ],
      quick_apps: null,
      detections: [
        {
          id: 'java-maven',
          name: 'Java (Maven)',
          markers: ['pom.xml'],
        },
        {
          id: 'java-gradle',
          name: 'Java (Gradle)',
          markers: ['build.gradle', 'build.gradle.kts'],
        },
      ],
      health_checks: [],
    },
  }

  const summary = buildPluginConsentSummary(javaManifest)

  assert.equal(summary.runtimes[0].source, 'github.com/adoptium/temurin21-binaries')
  assert.equal(summary.runtimes[0].sizeText, null)
  assert.ok(summary.detections)
  assert.equal(summary.detections.items.length, 2)
  assert.deepEqual(summary.detections.items[0].markers, ['pom.xml'])
  assert.deepEqual(summary.detections.items[1].markers, ['build.gradle', 'build.gradle.kts'])
})

test('buildPluginConsentSummary formats size when present and handles multiple hosts', () => {
  const multiManifest: PluginManifest = {
    id: 'multi',
    name: 'Multi Tool',
    version: '2.0.0',
    description: 'Plugin with multiple runtimes across different hosts and sizes',
    author: 'Tester',
    homepage: '',
    kind: 'declarative',
    permissions: ['download'],
    contributes: {
      runtimes: [
        {
          id: 'tool-a',
          name: 'Tool A',
          version: '1.0',
          url: 'https://github.com/org-a/repo-a/releases/download/v1.0/tool-a.zip',
          sha256: 'aaa',
          size: 47185920, // 45 MB
        },
        {
          id: 'tool-b',
          name: 'Tool B',
          version: '2.5',
          url: 'https://cdn.tools.net:8443/packages/tool-b.zip',
          sha256: 'bbb',
          size: '205 MB',
        },
      ],
      quick_apps: null,
      detections: [],
      health_checks: [],
    },
  }

  const summary = buildPluginConsentSummary(multiManifest)

  assert.equal(summary.runtimes.length, 2)
  assert.equal(summary.runtimes[0].source, 'github.com/org-a/repo-a')
  assert.equal(summary.runtimes[0].sizeText, 'about 45 MB')

  assert.equal(summary.runtimes[1].source, 'cdn.tools.net:8443/packages')
  assert.equal(summary.runtimes[1].sizeText, 'about 205 MB')
})
