#!/bin/sh
set -eu
cd "$(dirname "$0")"
if [ "${1:-}" = "--sdk-only" ]; then
  exec ./build-sdk-apk.sh
fi
if ! command -v gradle >/dev/null 2>&1; then
  echo 'Gradle 8.9 and JDK 17 are required. Open this folder in Android Studio or use the Build TV APK GitHub Actions workflow.' >&2
  exit 1
fi
: "${ANDROID_HOME:=${ANDROID_SDK_ROOT:-}}"
if [ -z "$ANDROID_HOME" ]; then
  echo 'Set ANDROID_HOME to an Android SDK with platform 35 installed.' >&2
  exit 1
fi
export ANDROID_HOME
gradle --no-daemon assembleDebug lintDebug
find "$PWD/build/outputs/apk/debug" -name "*.apk" -print
