#!/usr/bin/env bash
# Mac 앱 안 업데이트(tauri-plugin-updater)가 읽는 latest.json을 만든다.
# 앱은 releases/latest/download/latest.json을 읽고, 받은 파일의 .sig를 앱에 박힌 공개 키로 확인한다(docs/release.md)
set -euo pipefail
: "${TAG:?}" "${GITHUB_REPOSITORY:?}"
version=${TAG#v}
base="https://github.com/$GITHUB_REPOSITORY/releases/download/$TAG"
dir=$(mktemp -d)
gh release download "$TAG" --repo "$GITHUB_REPOSITORY" -D "$dir" -p '*.sig'

mac="leaf_${version}_aarch64.app.tar.gz"
if [ ! -f "$dir/$mac.sig" ]; then echo "$mac.sig가 없습니다"; exit 1; fi
jq -n --arg v "$version" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg notes "https://github.com/$GITHUB_REPOSITORY/releases/tag/$TAG" \
  --arg sig "$(cat "$dir/$mac.sig")" --arg url "$base/$mac" \
  '{version: $v, notes: $notes, pub_date: $date, platforms: {"darwin-aarch64": {signature: $sig, url: $url}}}' > latest.json
cat latest.json
