#!/usr/bin/env bash
# Build the screen reader that runs on the phone (src/pachibo/Dumper.java) to
# pachibo-dumper.dex beside this script, which the crate embeds. Needs a JDK
# (javac) and a dexer: d8 (Android build-tools) where it is found, else dx
# (Debian/Ubuntu's dalvik-exchange). The Java reaches Android only by
# reflection, so no Android SDK is needed to compile it.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

javac --release 8 -Xlint:all -Werror -d "$work/classes" "$here/src/pachibo/Dumper.java"
(cd "$work/classes" && jar cf "$work/classes.jar" .)

if command -v d8 >/dev/null 2>&1; then
  d8 --min-api 21 --output "$work" "$work/classes.jar"
elif command -v dalvik-exchange >/dev/null 2>&1; then
  dalvik-exchange --dex --min-sdk-version=21 --output="$work/classes.dex" "$work/classes.jar"
elif command -v dx >/dev/null 2>&1; then
  dx --dex --min-sdk-version=21 --output="$work/classes.dex" "$work/classes.jar"
else
  echo "no dexer found: install d8 (Android build-tools) or dalvik-exchange" >&2
  exit 1
fi

cp "$work/classes.dex" "$here/pachibo-dumper.dex"
echo "built $here/pachibo-dumper.dex ($(wc -c < "$here/pachibo-dumper.dex") bytes)"
