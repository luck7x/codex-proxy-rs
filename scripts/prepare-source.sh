#!/usr/bin/env bash
# 拉取上游指定 tag 并按文件名顺序应用 patches/ 下的热补丁
# 用法：scripts/prepare-source.sh <上游 tag> <输出目录>
set -euo pipefail

TAG="${1:?usage: prepare-source.sh <upstream-tag> <dest>}"
DEST="${2:?usage: prepare-source.sh <upstream-tag> <dest>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UPSTREAM="${CPR_UPSTREAM_REPO:-https://github.com/zyycn/codex-proxy-rs.git}"
BASE="$(tr -d '[:space:]' < "$ROOT/patches/BASE")"

[ -e "$DEST" ] && { echo "目标目录已存在：$DEST" >&2; exit 1; }
git init -q "$DEST"
git -C "$DEST" remote add upstream "$UPSTREAM"

# 同时取补丁基线版本，补丁上下文变化时 3-way 合并需要基线文件
refspecs=("+refs/tags/$TAG:refs/tags/$TAG")
[ "$BASE" != "$TAG" ] && refspecs+=("+refs/tags/$BASE:refs/tags/$BASE")
git -C "$DEST" fetch -q --depth 1 upstream "${refspecs[@]}"
git -C "$DEST" -c advice.detachedHead=false checkout -q "refs/tags/$TAG"

shopt -s nullglob
patches=("$ROOT"/patches/*.patch)
[ "${#patches[@]}" -gt 0 ] || { echo "patches/ 下没有补丁" >&2; exit 1; }
for patch in "${patches[@]}"; do
  name="$(basename "$patch")"
  if git -C "$DEST" apply --3way --whitespace=nowarn "$patch"; then
    echo "已应用 $name"
  else
    echo "::error::补丁 $name 无法应用到 $TAG（基线 $BASE），按 README 的「补丁冲突」手动适配"
    git -C "$DEST" status --short >&2 || true
    exit 1
  fi
done
git -C "$DEST" diff --cached --stat
