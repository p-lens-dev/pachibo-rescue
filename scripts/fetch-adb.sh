#!/usr/bin/env bash
# Download Android Platform Tools' adb and stage it under src-tauri/binaries/,
# for the release build to carry it as a resource (so a person needs no adb of
# their own). Pass the OS to stage for — linux, mac or windows — or none to use
# this computer's. Windows also needs adb's two DLLs beside it.
set -euo pipefail

os="${1:-}"
if [ -z "$os" ]; then
  case "$(uname -s)" in
    Linux*) os=linux ;;
    Darwin*) os=mac ;;
    MINGW* | MSYS* | CYGWIN*) os=windows ;;
    *) echo "unknown OS; pass linux, mac or windows" >&2; exit 1 ;;
  esac
fi

case "$os" in
  linux) zip=platform-tools-latest-linux.zip ;;
  mac) zip=platform-tools-latest-darwin.zip ;;
  windows) zip=platform-tools-latest-windows.zip ;;
  *) echo "unknown OS '$os'; pass linux, mac or windows" >&2; exit 1 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/src-tauri/binaries"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "Fetching ${zip}..."
curl -fsSL -o "${work}/pt.zip" "https://dl.google.com/android/repository/${zip}"
if command -v unzip >/dev/null 2>&1; then
  unzip -q "$work/pt.zip" -d "$work"
else
  python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$work/pt.zip" "$work"
fi

mkdir -p "$out"
if [ "$os" = windows ]; then
  cp "$work/platform-tools/adb.exe" "$out/adb.exe"
  cp "$work/platform-tools/AdbWinApi.dll" "$out/AdbWinApi.dll"
  cp "$work/platform-tools/AdbWinUsbApi.dll" "$out/AdbWinUsbApi.dll"
else
  cp "$work/platform-tools/adb" "$out/adb"
  chmod +x "$out/adb"
fi

echo "Staged adb for ${os} in ${out}:"
ls -l "$out"
