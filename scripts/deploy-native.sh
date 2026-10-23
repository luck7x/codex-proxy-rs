#!/usr/bin/env bash
# 下载 Native Build 产物并部署为独立版本目录，切换 current 软链接后重启服务
# 旧版本目录保留，失败时按提示切回
# 用法：sudo scripts/deploy-native.sh <Actions run ID> <版本号，如 3.29.7+luck7x.1>
set -euo pipefail

RUN_ID="${1:?usage: deploy-native.sh <run-id> <version>}"
VERSION="${2:?usage: deploy-native.sh <run-id> <version>}"
REPO="${CPR_REPO:-luck7x/codex-proxy-rs}"
BASE="${CPR_BASE:-/opt/codex-proxy-rs-native}"
SERVICE="${CPR_SERVICE:-codex-proxy-rs}"
OWNER="${CPR_OWNER:-cpr}"

OLD="$(readlink -f "$BASE/current")"
NEW="$BASE-$VERSION"
[ -e "$NEW" ] && { echo "目标目录已存在：$NEW" >&2; exit 1; }
PORT="$(systemctl show -p Environment --value "$SERVICE" | tr ' ' '\n' | sed -n 's/^CPR_SERVER_PORT=//p')"
PORT="${PORT:-8080}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
gh run download "$RUN_ID" -R "$REPO" -n "codex-proxy-rs-native-$VERSION" -D "$WORK"
TARBALL="codex-proxy-rs_${VERSION}_linux_amd64.tar.gz"
expected="$(awk -v name="$TARBALL" '{n=$2; sub(/.*\//, "", n)} n == name {print $1}' "$WORK/checksums.txt")"
actual="$(sha256sum "$WORK/$TARBALL" | awk '{print $1}')"
[ -n "$expected" ] && [ "$expected" = "$actual" ] || { echo "SHA-256 不一致：$expected / $actual" >&2; exit 1; }
mkdir "$WORK/extract"
tar -xzf "$WORK/$TARBALL" -C "$WORK/extract"
PKG="$WORK/extract/codex-proxy-rs"
[ "$("$PKG/codex-proxy-rs" --version)" = "$VERSION" ] || { echo "程序版本与 $VERSION 不一致" >&2; exit 1; }

# 程序、前端与配套官方插件清单来自同一构建，配置与运行数据沿用当前版本
install -d -o "$OWNER" -g "$OWNER" -m 0755 "$NEW" "$NEW/deploy"
cp -r "$PKG/codex-proxy-rs" "$PKG/web" "$PKG/plugins" "$NEW/"
cp "$PKG/deploy/config.example.yaml" "$NEW/deploy/"
cp -p "$OLD/deploy/config.yaml" "$NEW/deploy/config.yaml"
ln -s "$BASE/runtime" "$NEW/.runtime"
chown -R -h "$OWNER:$OWNER" "$NEW"
chmod 0640 "$NEW/deploy/config.yaml"
chmod 0755 "$NEW/codex-proxy-rs"
chmod 0555 "$NEW/plugins/official"
chmod 0444 "$NEW/plugins/official/plugin-release-manifest.json"

echo "$OLD" > "$BASE/previous-current.txt"
ln -sfn "$NEW" "$BASE/current.next"
mv -T "$BASE/current.next" "$BASE/current"
systemctl restart "$SERVICE"

code=""
for _ in $(seq 1 60); do
  code="$(curl -sS --max-time 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/healthz" 2>/dev/null || true)"
  [ "$code" = 204 ] && break
  sleep 1
done
if [ "$code" != 204 ]; then
  echo "健康检查失败（$code），回退：ln -sfn $OLD $BASE/current && systemctl restart $SERVICE" >&2
  exit 1
fi
echo "已部署 $("$BASE/current/codex-proxy-rs" --version)：$NEW，上一版本 $OLD"
