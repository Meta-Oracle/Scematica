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
let changed = false

if (src.includes(MARKER)) {
  console.log('[signing] signing block already present — skipping.')
} else {
  changed = true
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

if (!src.includes(MARKER)) {
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
}

// Version the app from web/package.json (single source of truth): drives versionName,
// versionCode (semver -> N), and the release artifact name.
const VERSION_MARKER = '// scematica-version'
if (!src.includes(VERSION_MARKER)) {
  // The fallback is deliberately 0.0.0/1, not a plausible-looking version. If reading
  // package.json ever fails, a build that silently ships the wrong-but-believable
  // version is far worse than one that is obviously broken.
  const versionBlock = `${VERSION_MARKER}
def scematicaVersionName = "0.0.0"
def scematicaVersionCode = 1
try {
    def pkgText = new File(rootProject.projectDir, "../package.json").getText("UTF-8")
    def vm = (pkgText =~ /"version"\\s*:\\s*"([^"]+)"/)
    if (vm.find()) {
        scematicaVersionName = vm.group(1)
        scematicaVersionCode = scematicaVersionName.tokenize('.').inject(0) { acc, part -> acc * 100 + (part as int) }
    }
} catch (Exception e) {
    logger.error("scematica: could not read web/package.json version — falling back to \${scematicaVersionName}. This apk is NOT correctly versioned.")
}

`
  src = src.replace(/android\s*\{/, `${versionBlock}android {`)
  src = src.replace(/versionCode\s+\d+/, 'versionCode scematicaVersionCode')
  src = src.replace(/versionName\s+"[^"]*"/, 'versionName scematicaVersionName')
  changed = true
  console.log('[signing] versioned from web/package.json')
}

// Name the release artifact scematica-v<version>.apk (not app-release.apk).
const NAME_MARKER = '// scematica-apk-name'
if (!src.includes(NAME_MARKER)) {
  src += `\n${NAME_MARKER}
android.applicationVariants.all { variant ->
    if (variant.buildType.name == "release") {
        variant.outputs.all { output ->
            output.outputFileName = "scematica-v\${scematicaVersionName}.apk"
        }
    }
}
`
  changed = true
  console.log('[signing] release output name set to scematica-v<version>.apk')
}

if (changed) {
  writeFileSync(gradle, src)
  console.log('[signing] patched android/app/build.gradle.')
} else {
  console.log('[signing] build.gradle already fully patched — nothing to do.')
}
