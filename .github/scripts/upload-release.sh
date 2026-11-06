#!/usr/bin/env bash
# 빌드가 끝난 작업마다 부른다. Release가 없으면 초안으로 만들고 받은 파일을 올린다(release.yml).
# 초안이라 파일이 다 붙기 전에는 releases/latest가 바뀌지 않는다. 다 붙으면 publish 작업이 공개한다.
# 여러 작업이 동시에 만들려 하면 하나만 성공하므로, 만들기에 실패해도 Release가 있으면 그대로 올린다
set -euo pipefail
: "${TAG:?}" "${GITHUB_REPOSITORY:?}"
notes="$(dirname "$0")/../release-notes.md"
if ! gh release view "$TAG" --repo "$GITHUB_REPOSITORY" >/dev/null 2>&1; then
  gh release create "$TAG" --repo "$GITHUB_REPOSITORY" --title "scanshelf $TAG" --notes-file "$notes" --draft \
    || gh release view "$TAG" --repo "$GITHUB_REPOSITORY" >/dev/null
fi
printf '%s\n' "$@"
gh release upload "$TAG" "$@" --repo "$GITHUB_REPOSITORY" --clobber
