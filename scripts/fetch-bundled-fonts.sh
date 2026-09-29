#!/usr/bin/env sh
# Fetch the OFL fonts that back the optional bundled-fallback features.
#
# They are not committed: the CJK source is 17 MB, and a binary that large in git
# makes every clone pay for a face most builds never use. The Hebrew face is only
# 112 KB and *is* committed, so a default build needs nothing from this script.
#
#   sh scripts/fetch-bundled-fonts.sh          # both
#   sh scripts/fetch-bundled-fonts.sh hebrew   # just the Hebrew face
#   sh scripts/fetch-bundled-fonts.sh cjk      # just the CJK face
#
# Both are SIL Open Font License 1.1; the licence text lands beside the font.

set -eu

DIR="$(CDPATH='' cd -- "$(dirname -- "$0")/../crates/hikari-core/assets/fonts" && pwd)"
BASE="https://raw.githubusercontent.com/google/fonts/main/ofl"
want="${1:-all}"

fetch() {
  echo "  $2"
  curl -fsSL -o "$DIR/$2" "$BASE/$1/$3"
  printf '     %s bytes\n' "$(wc -c <"$DIR/$2" | tr -d ' ')"
}

mkdir -p "$DIR"

case "$want" in
hebrew)
  fetch notosanshebrew 'NotoSansHebrew.ttf' \
    'NotoSansHebrew%5Bwdth%2Cwght%5D.ttf'
  fetch notosanshebrew 'OFL-NotoSansHebrew.txt' 'OFL.txt'
  ;;
cjk)
  fetch notosanssc 'NotoSansSC.ttf' 'NotoSansSC%5Bwght%5D.ttf'
  fetch notosanssc 'OFL-NotoSansSC.txt' 'OFL.txt'
  ;;
all)
  "$0" hebrew
  "$0" cjk
  ;;
*)
  echo "usage: $0 [hebrew|cjk|all]" >&2
  exit 2
  ;;
esac

echo "done. bundled-cjk is ~10 MB after subsetting, so enable it only for"
echo "native deployments that need deterministic CJK."
