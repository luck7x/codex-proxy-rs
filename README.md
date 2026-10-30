# Codex Proxy RS 热补丁（luck7x）

本分支只保存热补丁、构建工作流和部署脚本，不包含上游源码。构建时由 GitHub Actions 拉取上游
[zyycn/codex-proxy-rs](https://github.com/zyycn/codex-proxy-rs) 的指定版本，按顺序打上 `patches/`
下的补丁，检查通过后编译 Linux amd64 发行包。所有编译都在 GitHub 上完成，服务器只负责下载和部署。

- 工作流只能手动运行，没有任何自动触发
- 上游原有的 CI、Security Scan 等工作流已在本仓库停用
- 产物版本号为 `<上游版本>+luck7x.<构建序号>`，例如 `3.29.7+luck7x.1`

## 当前补丁

| 文件 | 作用 |
| --- | --- |
| `0001-account-warmup-storage-api.patch` | 账号预激活排期的独立表、管理接口 `/api/admin/accounts/warmup` 与审计 |
| `0002-openai-account-warmup-schedule.patch` | 预激活按账号排期；修正把未开始的 5h 窗口当成活跃窗口而跳过的问题 |
| `0003-account-warmup-ui.patch` | 账号额度面板的预激活设置 |

`patches/BASE` 记录补丁当前基于的上游版本，补丁冲突时用于 3-way 合并。

### 账号预激活用法

总开关、预激活模型和全局时间仍在管理端「设置 → 账号预激活」。每个账号在账号列表展开后的额度面板里设置：

| 方式 | 效果 |
| --- | --- |
| 跟随全局 | 按设置页的执行时间预激活 |
| 自定义 | 只在列出的时间预激活，每个账号最多 8 个 |
| 关闭 | 该账号不参与预激活 |

- 需要失败补发时多加一个时间，例如 `06:00`、`06:05`；第一次成功时后一次只多发一个小请求，不会重启窗口
- 时间按部署时区（`host.timezone`）计算
- 仍会跳过周额度已用完的账号，以及 5h 窗口正在计时且距重置超过 30 分钟的账号
- 账号备注不参与预激活

排期存在 `luck7x_account_warmup` 表：没有记录的账号跟随全局，`schedule_time` 为空表示关闭。这张表由程序启动时自动创建，不进入上游迁移集合，所以不占上游的迁移编号；随业务备份一起备份，回退官方版本时留在库里不起作用。

上游对还没开始的 5h 窗口会返回「用量 0、重置时间 = 观测时刻 + 5 小时」的占位数据，原版会把它当作正在计时而跳过。
补丁识别这种占位（与观测时刻的偏差在 2 分钟内），照常预激活。

## 构建

GitHub 仓库页 → Actions → Native Build → Run workflow：

- `upstream_tag`：上游版本 tag，例如 `v3.29.7`
- `build`：同一上游版本第几次构建，从 `1` 开始；补丁有修改需要重新构建时加 1

也可以用命令行触发：

```bash
gh workflow run native-build.yml -R luck7x/codex-proxy-rs -f upstream_tag=v3.29.7 -f build=1
gh run list -R luck7x/codex-proxy-rs --workflow native-build.yml --limit 3
```

工作流步骤：

1. `prepare`：校验输入，拉取上游 tag 并应用补丁，从上游的 `rust-toolchain.toml` 和 `deploy/Dockerfile` 读取工具链版本
2. `checks`：`cargo fmt --check`、补丁涉及 crate 的 clippy 和相关测试，存储测试使用工作流启动的 PostgreSQL 与 Redis
3. `frontend`、`backend`：检查并编译管理端前端，编译后端程序，后端使用与上游发行相同的 Rust 镜像
4. `package`：检查通过后打包为 `codex-proxy-rs-native-<版本>` 产物，保留 30 天

## 部署

在服务器上执行（需要 root 和已登录的 `gh`）：

```bash
sudo scripts/deploy-native.sh <Actions run ID> <版本号>
# 例如
sudo scripts/deploy-native.sh 39935862450 3.29.7+luck7x.1
```

脚本会：

1. 下载产物并核对 SHA-256 和程序版本号
2. 安装到新目录 `/opt/codex-proxy-rs-native-<版本号>`，沿用当前版本的 `deploy/config.yaml` 和运行数据
3. 把 `/opt/codex-proxy-rs-native/current` 切换到新目录并重启服务，等待 `/healthz` 返回 204

旧版本目录保留不删，上一版本路径记在 `/opt/codex-proxy-rs-native/previous-current.txt`。回退：

```bash
sudo ln -sfn "$(cat /opt/codex-proxy-rs-native/previous-current.txt)" /opt/codex-proxy-rs-native/current
sudo systemctl restart codex-proxy-rs
```

路径和服务名可用环境变量覆盖：`CPR_BASE`、`CPR_SERVICE`、`CPR_OWNER`、`CPR_REPO`。健康检查端口从 systemd 服务的
`CPR_SERVER_PORT` 读取。

## 同步上游新版本

1. 查看上游最新版本和发布说明，留意数据库迁移、配置字段和插件协议的变化：

   ```bash
   gh release view -R zyycn/codex-proxy-rs
   ```

2. 用新 tag 运行 Native Build，`build` 填 `1`
3. 补丁能干净打上且检查通过，就得到新版本发行包；升级前按上游说明备份数据库，再用部署脚本部署。新版本自带的数据库迁移会在启动时自动执行
4. 部署成功后把 `patches/BASE` 改成新 tag 并提交，作为以后 3-way 合并的基线

不要在管理端使用「在线更新」：它会换成官方构建，热补丁随之失效。

### 补丁冲突

`prepare` 步骤报「补丁无法应用」时，说明上游改动了补丁涉及的代码，需要手动适配。适配只用 git，不需要在本地编译：

```bash
git clone https://github.com/zyycn/codex-proxy-rs.git cpr-upstream && cd cpr-upstream
git checkout -b adapt <新 tag>
# 依次处理三个补丁，每个补丁打上并解决冲突后提交一次，记下提交哈希
git apply --3way /path/to/hotpatch/patches/0001-account-warmup-storage-api.patch
git add -A && git commit -m "adapt 0001"  # 记为 $A
git apply --3way /path/to/hotpatch/patches/0002-openai-account-warmup-schedule.patch
git add -A && git commit -m "adapt 0002"  # 记为 $B
git apply --3way /path/to/hotpatch/patches/0003-account-warmup-ui.patch
git add -A && git commit -m "adapt 0003"  # 记为 $C

# 从这三个提交重新生成补丁
git diff --full-index "$A^" "$A" > /path/to/hotpatch/patches/0001-account-warmup-storage-api.patch
git diff --full-index "$B^" "$B" > /path/to/hotpatch/patches/0002-openai-account-warmup-schedule.patch
git diff --full-index "$C^" "$C" > /path/to/hotpatch/patches/0003-account-warmup-ui.patch
```

把补丁文件开头的说明行补回去（`git apply` 会忽略第一个 `diff --git` 之前的文字），提交到本分支，再运行 Native Build。
编译和测试是否通过以 Actions 结果为准。

冲突最可能出现在 `warmup.rs`、`workers.rs` 和账号路由；排期表、管理接口和前端组件都是独立的新文件，上游改动很难碰到。

## 新增补丁

1. 从上游 tag 拉分支修改代码，提交后生成补丁：

   ```bash
   git diff --full-index <tag> HEAD -- backend frontend > patches/0002-<简短说明>.patch
   ```

2. 补丁按文件名顺序应用，编号接在已有补丁之后
3. 补丁改动 `provider-openai` 以外的 crate 时，在工作流 `checks` 中补上对应的 clippy 和测试
4. 补丁改动前端时，部署脚本会同时换上新的 `web/dist`，无需额外处理

## 分支

| 分支 | 用途 |
| --- | --- |
| `hotpatch` | 默认分支，即本分支：补丁、工作流、脚本和文档 |
| `luck7x/account-warmup-settings` | 补丁在 v3.29.7 上的完整源码，供查看和比对，每个补丁一个提交 |
| `main` | 早期的 3.16 定制版本，已不再使用 |
