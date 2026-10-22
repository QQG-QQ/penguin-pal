# PenguinPal 0.4 动画包

右键企鹅，进入「动作」选择互动、生活或表情；「素材」管理动画包。0.5 内置 19 段透明短视频，覆盖面板 14 个动作，并保留逐帧回退。减少动态、安全模式显示静态预览；隐藏/全屏隐藏/贴边半隐藏时暂停视频。

## 导入

把 `manifest.json` 和素材放在同一文件夹，点击「导入动画包」选择该清单。程序检查并复制到 `%LOCALAPPDATA%/com.penguinpal.app/companion/packs/<id>/`，不会移动原文件。同名 id 不覆盖；取消选择不修改动画库。用「内置企鹅」可随时恢复默认。

示例采用自己的 PNG 帧：

```json
{
  "schema": 1,
  "id": "my-penguin",
  "name": "我的企鹅",
  "clips": [{
    "id": "wave",
    "slot": "greet",
    "weight": 1,
    "frameMs": 143,
    "loop": false,
    "poster": "wave-0.png",
    "frames": ["wave-0.png", "wave-1.png", "wave-2.png"]
  }]
}
```

视频动作把 `frames` 设为 `[]`，添加 `"video": "wave.webm"`，保留 `poster`。外部包的一个动作只能选 PNG 或 WebM。素材包缺少的动作沿用内置企鹅。包和动作 id 仅接受小写英文字母、数字和 `-`，最长 40 字符，排除 Windows 设备保留名；`builtin` 是内置包专用 id。图片/视频文件名遵循同样规则并加小写扩展名。

- PNG：256 × 256 RGBA，角色完整放入画布，脚底基线推荐 y=244，站立包围盒应与内置角色接近。程序保留 alpha，不自动去背景。
- WebM：推荐 VP9 + alpha，无音轨，256 × 256，时长大于 0 且不超过 60 秒。播放器检查尺寸/时长，8 秒未就绪或解码失败回退至预览图。视频须使用真正透明通道，绿色背景不自动抠图。
- `poster` 必须是同一动作的透明 PNG。视频期间的点击穿透使用预览图轮廓；动作大幅伸展的区域可能与轮廓不一致。
- `frameMs` 40–5000；`weight` 1–10。同一 slot 可放多个候选；存在其他候选时不连续选择同一个；空闲候选约每 12 秒轮换。非空闲动作在重新进入动作或手动再次点击时重新选择。
- 最多 32 个包，每包 64 个动作、每动作 120 帧；清单最多 128 KiB，单素材 32 MiB，单包 200 MiB。只接受同目录素材，不接受 URL、绝对路径、子目录、符号链接、压缩包或脚本。导入先检查文件头；实际可解码性由播放器进一步检查。
- 刷新会重新读取清单；已缓存的同名文件内容可能仍由 WebView2 缓存，修改素材后建议使用新文件名/id 再导入。删除不使用的包可在文件夹中手动进行，再刷新；内置素材不受影响。

支持 slot：`idle blink look think walk-left walk-right rest happy greet pat poke tail eat sweep wake talk annoyed dizzy land drag fall working waiting ended interrupted toolError`。

`working/waiting/ended/interrupted/toolError` 跟随现有 Codex 状态；可关闭「任务动作」保留文字状态。结束表示本轮结束，不声明任务成功。不会连接第二个模型，也不会读写 Codex 聊天或长期记忆。

## 参考来源和范围

动作池、连续动画切换和桌宠交互思路参考 [PC2005-cloud/dsh-pet](https://github.com/PC2005-cloud/dsh-pet) 的 `e17e89be92aaa7b71c4f3e9e89995a23a781d725`。本版独立实现，没有复制其非商业动画、提示词或源视频，也不直接兼容其 `config.jsonc`。企鹅图像由内置 imagegen 根据本项目角色生成，完整提示词见素材目录。

本版为单宠物；未实现多宠物碰撞和自动生成数百段动画。0.5 新增 16 组各 8 帧的自有动作、2 段复用旧帧的行走视频，并沿用原挥手形象。0.5.1 将全部 19 段内置视频改用 RIFE 离线插帧制作成 30 fps；新动作约 1.33 秒、40 帧，原 PNG 与图集保留。插值估计中间运动，不能增加原作画细节；不是 AI 视频模型生成的长连续动作。完整提示词、制作脚本用法和说明在 penguin/companion-v3/。外部包的视频不自动改写；如选择了旧示例包，挥手仍使用包内旧视频，选择「内置企鹅」可使用新版全部视频。

手动点选动作会在该动作期间优先播放，Codex 状态文字继续更新。外部包缺失的动作使用内置视频，路径由实际提供素材的包决定。暂停/切换造成的旧播放中断不会标成坏视频；真实解码或加载失败仍回退。再次点击动作会重试，并清除上一动作的视频提示。诊断只记录固定失败类型和时间，不保存素材路径。

额外的 `--smoke --smoke-preview --smoke-actions` 仅在隔离测试中依次播放 19 段视频，并在独立状态目录记录每段的解码回执；普通启动不会自动执行这个测试。测试时可将 PENGUINPAL_SMOKE_SECONDS 设为 55。

## 独立运行验收

`PenguinPal-Companion.exe --smoke --smoke-preview` 使用 `companion/smoke-preview-<进程号>/` 中的独立状态及独立 WebView2 缓存，循环预览挥手并打开面板，默认 3 秒后正常退出；可用 `PENGUINPAL_SMOKE_SECONDS` 设置 1–600 秒。此显式动画测试会播放视频，即使系统开启减少动态；普通陪伴仍尊重系统与桌宠的减少动态设置。该明确测试组合允许与正常桌宠同时运行；普通启动仍保持单实例。预览不安装 hooks，也不保存到正在使用的桌宠状态。

`scripts/measure-companion.ps1 -Executable <exe> -Output <json> -Seconds 30 -Preview` 测量独立预览的进程树。此数据对应持续视频和面板，不能与空闲待机占用直接比较；预览模式跳过正常单实例检查。`-Preview` 省略时沿用原有正常模式测量，并要求先退出其他桌宠。
