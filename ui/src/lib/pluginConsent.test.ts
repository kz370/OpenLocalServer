import test from 'node:test'
import assert from 'node:assert/strict'
import type { PluginManifest } from '../core/types/system.ts'
import { buildPluginConsentSummary, extractHostname } from './pluginConsent.ts'

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
    extractHostname('https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12%2B1/OpenJDK21U-jdk_x64_windows_hotspot_21.0.12.1_1.zip'),
    'github.com'
  )
  assert.equal(
    extractHostname('https://builds.dotnet.microsoft.com/dotnet/Sdk/10.0.401/dotnet-sdk-10.0.401-win-x64.zip'),
    'builds.dotnet.microsoft.com'
  )
  assert.equal(extractHostname(''), '')
  assert.equal(extractHostname('invalid-url'), '')
})

test('buildPluginConsentSummary formats Go manifest correctly', () => {
  const goManifest: PluginManifest = {
    id: 'go',
    name: 'Go',
    version: '1.0.0',
    description: 'The Go toolchain, from go.dev. Installs under Runtimes; nothing else changes.',
    author: 'OpenLocalServer',
    homepage: '',
    kind: 'declarative',
    permissions: ['download', 'read_projects'],
    contributes: {
      runtimes: [
        {
          id: 'go',
          name: 'Go',
          version: '1.27.1',
          url: 'https://go.dev/dl/go1.27.1.windows-amd64.zip',
          sha256: 'a3911b5e0e1b1053f25ed0675f4c1c6aad1e2bfcf253df2b9be4caabd2edd95d',
        },
      ],
      quick_apps: null,
      detections: [
        {
          id: 'go-project',
          name: 'Go',
          markers: ['go.mod'],
        },
      ],
      health_checks: [],
    },
  }

  const summary = buildPluginConsentSummary(goManifest)

  // Runtimes
  assert.equal(summary.runtimes.length, 1)
  assert.equal(summary.runtimes[0].name, 'Go')
  assert.equal(summary.runtimes[0].version, '1.27.1')
  assert.equal(summary.runtimes[0].host, 'go.dev')
  assert.match(summary.runtimes[0].explanation, /SHA-256/)
  assert.match(summary.runtimes[0].explanation, /Runtimes page/)

  // Detections
  assert.ok(summary.detections)
  assert.equal(summary.detections.items.length, 1)
  assert.equal(summary.detections.items[0].name, 'Go')
  assert.deepEqual(summary.detections.items[0].markers, ['go.mod'])
  assert.match(summary.detections.explanation, /Reads files only/)

  // Empty sections omitted
  assert.equal(summary.quickApps, null)
  assert.equal(summary.healthChecks.length, 0)

  // Permissions & closing
  assert.deepEqual(summary.rawPermissions, ['download', 'read_projects'])
  assert.match(summary.closingSentence, /Runtimes page/)
  assert.match(summary.closingSentence, /Plugins page/)
})

test('buildPluginConsentSummary formats Java manifest with multiple detections', () => {
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

  // Runtimes
  assert.equal(summary.runtimes.length, 1)
  assert.equal(summary.runtimes[0].name, 'Java (Temurin JDK)')
  assert.equal(summary.runtimes[0].version, '21.0.12.1')
  assert.equal(summary.runtimes[0].host, 'github.com')

  // 2 detections
  assert.ok(summary.detections)
  assert.equal(summary.detections.items.length, 2)
  assert.equal(summary.detections.items[0].name, 'Java (Maven)')
  assert.deepEqual(summary.detections.items[0].markers, ['pom.xml'])
  assert.equal(summary.detections.items[1].name, 'Java (Gradle)')
  assert.deepEqual(summary.detections.items[1].markers, ['build.gradle', 'build.gradle.kts'])
})

test('buildPluginConsentSummary omits empty sections for manifest with no downloads or detections', () => {
  const minimalManifest: PluginManifest = {
    id: 'custom-minimal',
    name: 'Minimal Plugin',
    version: '0.1.0',
    description: 'A plugin that only contributes health checks',
    author: 'Someone',
    homepage: '',
    kind: 'declarative',
    permissions: ['network'],
    contributes: {
      runtimes: [],
      quick_apps: null,
      detections: [],
      health_checks: [
        {
          id: 'chk',
          name: 'Local API',
          kind: 'http',
          target: 'http://127.0.0.1:8080/health',
          hint: null,
        },
      ],
    },
  }

  const summary = buildPluginConsentSummary(minimalManifest)

  // Runtimes and detections are empty / omitted
  assert.equal(summary.runtimes.length, 0)
  assert.equal(summary.detections, null)
  assert.equal(summary.quickApps, null)

  // Health checks present
  assert.equal(summary.healthChecks.length, 1)
  assert.equal(summary.healthChecks[0].name, 'Local API')
  assert.equal(summary.healthChecks[0].target, 'HTTP http://127.0.0.1:8080/health')

  // Closing sentence does not claim runtimes exist
  assert.doesNotMatch(summary.closingSentence, /runtimes appear/)
  assert.match(summary.closingSentence, /active/)
  assert.match(summary.closingSentence, /Plugins page/)
})

test('buildPluginConsentSummary ignores author free-text to prevent spoofing', () => {
  const maliciousManifest: PluginManifest = {
    id: 'untrusted',
    name: 'Deceptive Plugin',
    version: '1.0.0',
    description: 'MISLEADING TEXT: This plugin is completely safe and requires no permissions at all!',
    author: 'Attacker',
    homepage: '',
    kind: 'declarative',
    permissions: ['download'],
    contributes: {
      runtimes: [
        {
          id: 'bad-runtime',
          name: 'Trojan',
          version: '1.0',
          url: 'https://evil.example.com/bad.zip',
          sha256: '0000000000000000000000000000000000000000000000000000000000000000',
        },
      ],
      quick_apps: null,
      detections: [],
      health_checks: [],
    },
  }

  const summary = buildPluginConsentSummary(maliciousManifest)

  // The deceptive description is NOT anywhere in the generated summary
  const summaryJson = JSON.stringify(summary)
  assert.equal(summaryJson.includes('MISLEADING TEXT'), false)
  assert.equal(summary.runtimes[0].host, 'evil.example.com')
})
