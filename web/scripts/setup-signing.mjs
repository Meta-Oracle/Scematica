#!/usr/bin/env node
// Wire release signing into the Capacitor Android project.
//
// Run AFTER `npx cap add android`. Idempotent: it injects a `signingConfigs.release`
// (reading android/keystore.properties) into android/app/build.gradle and points the
// release buildType at it. If keystore.properties is missing it writes a template you
// fill in. Keeps signing secrets out of the repo (.gitignore covers them).
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const root = process.cwd()
const gradle = join(root, 'android', 'app', 'build.gradle')
const props = join(root, 'android', 'keystore.properties')
const MARKER = '// scematica-signing'

if (!existsSync(gradle)) {
  console.error('[signing] android/app/build.gradle not found — run `npx cap add android` first.')
  process.exit(1)
}

if (!existsSync(props)) {
  writeFileSync(
    props,
    [
      '# Fill these in, then re-run `npm run mobile:signing`. This file is git-ignored.',
      'storeFile=../../scematica-release.jks',
      'storePassword=CHANGEME',
      'keyAlias=scematica',
      'keyPassword=CHANGEME',
      '',
    ].join('\n'),
  )
  console.log('[signing] wrote android/keystore.properties template — fill it in.')
}

let src = readFileSync(gradle, 'utf8')
if (src.includes(MARKER)) {
  console.log('[signing] build.gradle already patched — nothing to do.')
  process.exit(0)
}

const loader = `${MARKER}
def keystorePropertiesFile = rootProject.file("keystore.properties")
def keystoreProperties = new Properties()
if (keystorePropertiesFile.exists()) {
    keystoreProperties.load(new FileInputStream(keystorePropertiesFile))
}

`

const signingBlock = `    signingConfigs {
        release {
            if (keystorePropertiesFile.exists()) {
                storeFile file(keystoreProperties['storeFile'])
                storePassword keystoreProperties['storePassword']
                keyAlias keystoreProperties['keyAlias']
                keyPassword keystoreProperties['keyPassword']
            }
        }
    }
`

// Prepend the properties loader above the `android {` block.
src = src.replace(/android\s*\{/, `${loader}android {`)

// Insert signingConfigs right after the `android {` opening brace.
src = src.replace(/android\s*\{\n/, (m) => `${m}${signingBlock}`)

// Point the release buildType at the signing config.
if (/buildTypes\s*\{\s*release\s*\{/.test(src)) {
  src = src.replace(/(buildTypes\s*\{\s*release\s*\{)/, `$1\n            signingConfig signingConfigs.release`)
} else {
  console.warn('[signing] could not find buildTypes.release — add `signingConfig signingConfigs.release` manually.')
}

writeFileSync(gradle, src)
console.log('[signing] patched android/app/build.gradle → release signing wired.')
