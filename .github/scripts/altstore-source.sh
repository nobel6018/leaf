#!/usr/bin/env bash
# AltStore 소스(apps.json)를 만든다. release.yml ios 작업이 leaf-ios.ipa와 함께 Release에 올린다.
# 소스 URL은 releases/latest/download/apps.json으로 고정이라 AltStore가 늘 최신 Release를 읽는다(docs/release.md).
#
# AltStore는 소스에 적은 권한이 받은 ipa와 다르면 설치를 거부한다.
# 그래서 버전과 권한(…UsageDescription)은 손으로 적지 않고 빌드된 .app의 Info.plist에서 읽는다.
# 서명 없이 빌드하므로 바이너리에 entitlements가 없다(빈 배열)
#
# 쓰기: altstore-source.sh <leaf.app> <leaf-ios.ipa> <태그> > apps.json
set -euo pipefail
app=$1 ipa=$2 tag=$3
: "${GITHUB_REPOSITORY:?}"

plist="$app/Info.plist"
read_key() {
  /usr/bin/plutil -extract "$1" raw -o - "$2"
}

version=$(read_key CFBundleShortVersionString "$plist")
build=$(read_key CFBundleVersion "$plist")
bundle_id=$(read_key CFBundleIdentifier "$plist")
min_os=$(read_key MinimumOSVersion "$plist")
size=$(stat -f %z "$ipa")
repo_url="https://github.com/$GITHUB_REPOSITORY"

# 앱과 앱 확장(PlugIns/*.appex)의 사용 설명 키를 모두 모은다
privacy='{}'
while IFS= read -r -d '' p; do
  json=$(/usr/bin/plutil -convert json -o - "$p")
  privacy=$(jq -n --argjson acc "$privacy" --argjson info "$json" \
    '$acc + ($info | with_entries(select(.key | endswith("UsageDescription"))))')
done < <(find "$app" -name Info.plist \( -path "$app/Info.plist" -o -path '*.appex/Info.plist' \) -print0)

jq -n \
  --arg version "$version" \
  --arg build "$build" \
  --arg bundle "$bundle_id" \
  --arg min "$min_os" \
  --argjson size "$size" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg download "$repo_url/releases/download/$tag/$(basename "$ipa")" \
  --arg notes "$repo_url/releases/tag/$tag" \
  --arg icon "https://raw.githubusercontent.com/$GITHUB_REPOSITORY/main/design/logo-ios-1024.png" \
  --arg website "$repo_url" \
  --argjson privacy "$privacy" \
  '{
    name: "leaf",
    subtitle: "거치대 스캔, Mac 보정과 OCR",
    iconURL: $icon,
    website: $website,
    tintColor: "#4F7A3A",
    nsfw: false,
    news: [],
    apps: [{
      name: "leaf",
      bundleIdentifier: $bundle,
      developerName: "nobel6018",
      subtitle: "거치대에 올리고 종이만 바꿔 놓으면 찍힙니다",
      localizedDescription: "팜플렛과 책을 자동으로 찍어 같은 Wi-Fi의 Mac이나 iCloud Drive로 보냅니다. 보정, OCR, AI 정리는 Mac의 leaf가 합니다.",
      iconURL: $icon,
      tintColor: "#4F7A3A",
      category: "productivity",
      versions: [{
        version: $version,
        buildVersion: $build,
        date: $date,
        localizedDescription: ("바뀐 점: " + $notes),
        downloadURL: $download,
        size: $size,
        minOSVersion: $min
      }],
      appPermissions: {
        entitlements: [],
        privacy: $privacy
      }
    }]
  }'
