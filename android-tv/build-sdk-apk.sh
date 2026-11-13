#!/bin/sh
# Direct SDK build for offline hosts where Gradle cannot open its local sockets.
set -eu
cd "$(dirname "$0")"
: "${ANDROID_HOME:=${ANDROID_SDK_ROOT:-}}"
if [ -z "$ANDROID_HOME" ] && [ -f local.properties ]; then
    ANDROID_HOME=$(sed -n 's/^sdk.dir=//p' local.properties)
fi
: "${ANDROID_HOME:?Set ANDROID_HOME to the Android SDK}"
: "${JAVA_HOME:?Set JAVA_HOME to a JDK (17 or newer)}"
export JAVA_HOME
PATH="$JAVA_HOME/bin:$PATH"
export PATH
SDK_TOOLS="$ANDROID_HOME/build-tools/35.0.0"
PLATFORM="$ANDROID_HOME/platforms/android-35/android.jar"
OUT="build/sdk-apk"
rm -rf "$OUT"
mkdir -p "$OUT/classes" "$OUT/dex" "$OUT/generated"
"$SDK_TOOLS/aapt2" compile --dir src/main/res -o "$OUT/resources.zip"
python3 - "$OUT/AndroidManifest.xml" <<'PY'
import sys, xml.etree.ElementTree as ET
ET.register_namespace('android', 'http://schemas.android.com/apk/res/android')
tree = ET.parse('src/main/AndroidManifest.xml')
r = tree.getroot()
r.set('package', 'local.tvbridge.companion')
r.set('{http://schemas.android.com/apk/res/android}versionCode', '3')
r.set('{http://schemas.android.com/apk/res/android}versionName', '0.2.1')
tree.write(sys.argv[1], encoding='utf-8', xml_declaration=True)
PY
"$SDK_TOOLS/aapt2" link -o "$OUT/unsigned.apk" -I "$PLATFORM" --manifest "$OUT/AndroidManifest.xml" --java "$OUT/generated" --min-sdk-version 26 --target-sdk-version 35 "$OUT/resources.zip"
find src/main/java "$OUT/generated" -name '*.java' > "$OUT/sources.txt"
javac --release 17 -classpath "$PLATFORM" -d "$OUT/classes" @"$OUT/sources.txt"
jar cf "$OUT/classes.jar" -C "$OUT/classes" .
"$SDK_TOOLS/d8" --lib "$PLATFORM" --min-api 26 --output "$OUT/dex" "$OUT/classes.jar"
python3 - "$OUT" <<'PY'
import sys, zipfile
from pathlib import Path
out = Path(sys.argv[1])
with zipfile.ZipFile(out / 'unsigned.apk', 'a') as apk:
    for dex in sorted((out / 'dex').glob('*.dex')):
        apk.write(dex, dex.name)
PY
"$SDK_TOOLS/zipalign" -f 4 "$OUT/unsigned.apk" "$OUT/aligned.apk"
# Keep this local signing key for future updates; never include it in source ZIPs.
KEYSTORE="${TV_BRIDGE_KEYSTORE:-$PWD/.signing/debug.keystore}"
if [ ! -f "$KEYSTORE" ]; then
    mkdir -p "$(dirname "$KEYSTORE")"
    keytool -genkeypair -keystore "$KEYSTORE" -storepass android -keypass android -alias androiddebugkey -dname 'CN=Android Debug,O=Android,C=US' -keyalg RSA -keysize 2048 -validity 10000
fi
mkdir -p ../releases
APK="$PWD/../releases/tv-bridge-companion.apk"
"$SDK_TOOLS/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android --key-pass pass:android --out "$APK" "$OUT/aligned.apk"
"$SDK_TOOLS/apksigner" verify --verbose "$APK"
"$SDK_TOOLS/aapt2" dump badging "$APK"
(cd ../releases && sha256sum tv-bridge-companion.apk > tv-bridge-companion.apk.sha256)
printf '\nAPK: %s\n' "$APK"
