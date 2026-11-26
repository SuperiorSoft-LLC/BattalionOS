#!/usr/bin/env bash
# BOS-SCAN-CRASH-1 guard: every class a release APK's manifest names for reflection by Firebase ComponentDiscovery
# ("com.google.firebase.components:<class>" meta-data, e.g. ML Kit's registrars behind os_scan_code) must survive R8
# with its no-argument constructor and the ComponentRegistrar interface. Usage: check-release-apk.sh <apk> [build-tools dir]
set -euo pipefail
apk="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"   # absolute: the unzip below runs from a temp dir
bt="${2:-${ANDROID_HOME:?set ANDROID_HOME or pass the build-tools dir}/build-tools/$(ls "$ANDROID_HOME/build-tools" | sort -V | tail -1)}"
ext=""; [ -x "$bt/aapt2.exe" ] && ext=".exe"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
# BOS-CLEARTEXT-1 guard: apps reach the PC over ws://<LAN or Tailscale ip>; a release APK must permit cleartext.
man="$("$bt/aapt2$ext" dump xmltree --file AndroidManifest.xml "$(cygpath -w "$apk" 2>/dev/null || echo "$apk")")"
grep -q 'usesCleartextTraffic.*=true' <<<"$man" || { echo "check-release-apk: usesCleartextTraffic is not true in $apk (BOS-CLEARTEXT-1)"; exit 1; }
grep -q 'networkSecurityConfig' <<<"$man" || { echo "check-release-apk: no networkSecurityConfig in $apk (BOS-CLEARTEXT-1)"; exit 1; }
echo "ok       cleartext permitted (usesCleartextTraffic=true, networkSecurityConfig set)"
regs="$("$bt/aapt2$ext" dump xmltree --file AndroidManifest.xml "$(cygpath -w "$apk" 2>/dev/null || echo "$apk")" | grep -o '"com.google.firebase.components:[^"]*"' | sed 's/"com.google.firebase.components://; s/"$//' | sort -u)"
[ -n "$regs" ] || { echo "check-release-apk: no component registrars in the manifest of $apk (is ML Kit still linked?)"; exit 1; }
(cd "$tmp" && unzip -q -o "$apk" 'classes*.dex')
for d in "$tmp"/classes*.dex; do "$bt/dexdump$ext" "$(cygpath -w "$d" 2>/dev/null || echo "$d")"; done > "$tmp/dump.txt"
bad=0
for c in $regs; do
  desc="L$(echo "$c" | tr . /);"
  block="$(awk -v d="  Class descriptor  : '$desc'" '$0==d{on=1;print;next} on&&/^  Class descriptor/{exit} on{print}' "$tmp/dump.txt")"
  if [ -z "$block" ]; then echo "MISSING  $c (removed by R8)"; bad=1; continue; fi
  if ! grep -A3 "^  Interfaces" <<<"$block" | grep -q "Lcom/google/firebase/components/ComponentRegistrar;"; then echo "BROKEN   $c no longer implements ComponentRegistrar"; bad=1; fi
  if ! awk '/name *: .<init>./{getline; if ($0 ~ /\(\)V/) f=1} END{exit !f}' <<<"$block"; then echo "BROKEN   $c lost its no-argument constructor"; bad=1; fi
  [ $bad = 0 ] && echo "ok       $c"
done
[ $bad = 0 ] || { echo "check-release-apk: R8 stripped what ComponentDiscovery needs; see proguard-rules.pro (BOS-SCAN-CRASH-1)"; exit 1; }
