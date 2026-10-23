# Codex Proxy RS 热补丁（luck7x）

本分支只保存热补丁、构建工作流和部署脚本，不包含上游源码。
构建时由 GitHub Actions 拉取上游 [zyycn/codex-proxy-rs](https://github.com/zyycn/codex-proxy-rs) 的指定 tag，按文件名顺序应用 `patches/` 下的补丁，检查后编译 Linux amd64 发行包。
编译只在 GitHub Actions 上进行，服务器只负责下载、校验和切换版本

[当前补丁](#当前补丁) · [构建](#构建) · [部署](#部署) · [同步上游新版本](#同步上游新版本) · [补丁冲突](#补丁冲突) · [新增补丁](#新增补丁) · [回退](#回退) · [注意事项](#注意事项)

## 目录

| 路径 | 作用 |
| --- | --- |
| `patches/BASE` | 补丁最后一次适配的上游 tag，同时作为 3-way 合并的基线 |
| `patches/*.patch` | 热补丁，按文件名顺序应用 |
| `scripts/prepare-source.sh` | 拉取上游 tag 并应用全部补丁，CI 与本地适配共用；只用 git，不编译 |
| `scripts/deploy-native.sh` | 在服务器上下载 CI 产物、校验并切换版本 |
| `.github/workflows/native-build.yml` | 唯一启用的工作流，只能手动运行 |

## 当前补丁

### 0001-openai-account-warmup-schedule

账号预激活按账号单独定时，并修正空闲账号被误判为窗口计时中的问题

- 账号备注写 `预激活=07:30` 或 `预激活=07:30,12:30`，该账号只在这些本地时刻预激活；也可以写 `warmup=07:30`
- 备注写 `预激活=关闭`（或 `warmup=off`）时该账号不参与预激活
- 备注没有标记的账号跟随「设置 → 账号预激活」的全局时间，按原来的方式批量执行；标记格式错误时同样跟随全局时间并在日志告警
- 时间格式与全局设置一致：`HH:MM`，多个时间用逗号分隔，允许中文逗号和空格；标记前后可以有其他备注，用分号或换行隔开
- 总开关和预激活模型仍在设置页统一配置
- 上游对尚未开始计时的 5h 窗口返回「0 用量、重置时间 = 观测时刻 + 5h」的占位；原逻辑把它当成计时中的窗口而跳过，补丁后照常预激活。刚开始且用量仍显示 0 的窗口也会被这样识别，最多多发一次预激活请求，不会重新开始计时

预激活日志在 `runtime/logs/codex-proxy-rs-application.*.log`，按 `OpenAI account warmup` 搜索；`cycle started` 中的 `accounts` 是本轮到点的账号数，`global_due` 表示是否命中全局时间

## 构建

在 GitHub 的 **Actions → Native Build → Run workflow** 填写：

- `upstream_tag`：上游 tag，例如 `v3.29.7`
- `build`：同一上游版本的补丁构建序号，从 `1` 开始；补丁内容变化后重新构建同一上游版本时递增

产物版本为 `<版本>+luck7x.<序号>`，例如 `3.29.7+luck7x.1`；`+` 后的构建元数据不参与版本比较

命令行等价操作：

```bash
gh workflow run native-build.yml -R luck7x/codex-proxy-rs -f upstream_tag=v3.29.7 -f build=1
gh run watch -R luck7x/codex-proxy-rs "$(gh run list -R luck7x/codex-proxy-rs -w native-build.yml -L 1 --json databaseId -q '.[0].databaseId')"
```

工作流各任务：

| 任务 | 内容 |
| --- | --- |
| `prepare` | 校验输入、应用补丁、读取该上游版本声明的 Rust 与 Node 工具链，上传打好补丁的源码 |
| `checks` | `cargo fmt --check`、`provider-openai` 的 clippy（`-D warnings`）与额度相关测试 |
| `frontend` / `backend` | 编译管理端前端与后端程序，后端使用上游 Dockerfile 声明的 Rust 镜像 |
| `package` | 以上全部通过后打包 `codex-proxy-rs_<版本>_linux_amd64.tar.gz` 与 `checksums.txt` |

发行包保留 30 天，名称为 `codex-proxy-rs-native-<版本>`

## 部署

服务器需要已登录的 `gh`，以 root 运行：

```bash
git clone -b hotpatch https://github.com/luck7x/codex-proxy-rs.git codex-proxy-rs-hotpatch
sudo codex-proxy-rs-hotpatch/scripts/deploy-native.sh <run ID> 3.29.7+luck7x.1
```

脚本依次完成：

1. 下载该次运行的发行包，按 `checksums.txt` 校验 SHA-256，并确认程序报告的版本与参数一致
2. 解压到新目录 `/opt/codex-proxy-rs-native-<版本>`：程序、前端和配套的官方插件清单来自同一次构建，`deploy/config.yaml` 复制自当前版本，`.runtime` 指向共享运行目录
3. 原子切换 `/opt/codex-proxy-rs-native/current`，重启 `codex-proxy-rs` 服务，等待 `/healthz` 返回 204；端口从 systemd 单元的 `CPR_SERVER_PORT` 读取

目录、服务名、运行用户和仓库可用 `CPR_BASE`、`CPR_SERVICE`、`CPR_OWNER`、`CPR_REPO` 覆盖

## 同步上游新版本

1. 查看上游最新版本与发布说明，重点看数据库迁移和不兼容变更：`gh release view -R zyycn/codex-proxy-rs`
2. 按发布说明先备份数据库与配置；新版本的迁移在服务启动时自动执行，迁移后回退旧版本需要恢复备份
3. 运行 Native Build，`upstream_tag` 填新 tag、`build` 填 `1`
4. `prepare` 通过说明补丁能直接打上；全部任务通过后按[部署](#部署)切换
5. 确认新版本运行正常后，把 `patches/BASE` 改成新 tag 并提交，让后续 3-way 合并以新版本为基线

`prepare` 失败表示补丁冲突，按下一节处理；`checks` 或 `backend` 失败表示补丁能打上但与新版本代码不兼容，同样需要按下一节修改补丁

## 补丁冲突

本地只做 git 操作和代码编辑，编译与测试交给 CI：

```bash
git clone -b hotpatch https://github.com/luck7x/codex-proxy-rs.git codex-proxy-rs-hotpatch
cd codex-proxy-rs-hotpatch
scripts/prepare-source.sh v3.30.0 /tmp/cpr-v3.30.0     # 冲突时会列出未合并的文件
cd /tmp/cpr-v3.30.0
git status --short                                     # 解决冲突标记，保持补丁原有行为
git add -A
git diff --cached --full-index -- backend frontend > ~/codex-proxy-rs-hotpatch/patches/0001-openai-account-warmup-schedule.patch
```

保留补丁文件开头的说明段落（在第一行 `diff --git` 之前，应用时会被忽略），把其中的 `Base:` 改为新 tag，`patches/BASE` 同步修改，提交推送后重新运行 Native Build 验证

补丁改动的上游位置：

- `backend/crates/providers/openai/src/credential/quota/warmup.rs`：备注解析、到点账号选择、窗口占位识别
- `backend/crates/providers/openai/src/provider/workers.rs`：预激活定时任务按账号选择后领取执行时刻
- `backend/crates/providers/openai/tests/`：对应测试与内存仓库的备注辅助方法

上游如果自带了等价功能，删除对应补丁文件即可

## 新增补丁

1. 用 `scripts/prepare-source.sh <tag> <目录>` 得到已打好现有补丁的源码并提交为一个基线提交
2. 在其上修改，遵守上游 `AGENTS.md` 的约定（中文注释、测试随功能维护）
3. 用 `git diff --full-index <基线提交> -- backend frontend > patches/000N-<名称>.patch` 生成新补丁，文件名序号决定应用顺序
4. 改动涉及 `provider-openai` 以外的 crate 时，同步扩展工作流 `checks` 任务的 clippy 与测试范围
5. 在本文件「当前补丁」中说明新补丁的行为与配置方式

## 回退

旧版本目录在部署后保留，切回即可：

```bash
sudo ln -sfn /opt/codex-proxy-rs-native-<旧版本> /opt/codex-proxy-rs-native/current
sudo systemctl restart codex-proxy-rs
```

跨越数据库迁移的回退需要先恢复升级前的数据库备份

## 注意事项

- 不要使用管理端的「在线更新」：它会换成官方构建，热补丁随之丢失；升级一律走本仓库的构建与部署
- 发行包内的官方插件清单由工作流生成，插件列表为空，版本与 git sha 和程序一致；当前部署未安装插件，需要官方插件时另行评估
- 本仓库只启用 Native Build；从上游继承的 CI、Security Scan、Agent Instructions Guard、Release 已在仓库设置中停用，不会因推送、PR 或定时任务自动运行
- 分支 `luck7x/v3.29.7-account-warmup` 保存补丁在 v3.29.7 上的完整源码，仅供对照
