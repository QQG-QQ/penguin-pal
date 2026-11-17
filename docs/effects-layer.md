# 程序化特效层（CompanionFX）

桌宠的情绪符号、状态光环、场景天气和切换转场全部由 canvas 现场绘制，不使用任何图片素材。它由 `src/components/CompanionFX.vue` 提供，挂在 `.pet-visual` 内、角色层之上，`pointer-events: none`，不参与命中检测。

## 它接收什么

| Prop | 作用 |
| --- | --- |
| `paused` | 窗口隐藏、贴边半隐藏、全屏隐藏时为 true，暂停绘制但保留状态 |
| `reduced` | 减少动态 / 安全模式，粒子直接不生成 |
| `status` | Codex 状态：`working` `waiting` `ended` `interrupted` `toolError`，其余按 `idle` 处理 |
| `scene` | `none` / `rain` / `snow`，来自偏好设置 |
| `activity` | 当前动作，`rest` 时自动周期冒 Zzz |

## 它暴露什么

- `burst(kind)`：冒一次符号。kind 取值 `heart` `sparkle` `anger` `sweat` `question` `note` `zzz` `dust`
- `wipe()`：播放一次对角扫描光带（0.28 秒）

调用点都在 `CompanionApp.vue`：互动、拖拽、动作切换、Codex 状态。

## 触发对照

| 事件 | 表现 |
| --- | --- |
| 摸摸 | 爱心 ×4 |
| 戳戳（3 次以内 / 以上） | 问号 / 怒气符号 |
| 投喂、摸尾巴 | 星光 ×7 |
| 休息 | 每 0.75 秒一个 Z |
| 拖拽移动 | 汗滴（每次拖拽一次） |
| 进入 开心 / 挥手 | 星光 |
| 进入或离开 休息 / 表演 | 扫描转场 |
| Codex 工作中 | 脚下三段时间弧 + 三个轨道点 + 进度环 |
| 等待确认 | 琥珀色脉冲双环 |
| 出错 / 中断 | 红色抖动环 |
| 本轮结束 | 绿色扩散环 + 对勾 |

## 坐标系与约束

- 画布逻辑尺寸 250 × 250，按 `devicePixelRatio`（上限 2）放大，和角色显示尺寸一致
- 所有锚点是写死的逻辑坐标（例如头顶 `[125, 58]`、脚下 `[125, 236]`），改画布尺寸必须同步改锚点
- 粒子上限 140，超出丢弃最旧的；不使用阴影模糊，避免每帧重建滤镜
- 暂停时只跳过绘制，不清空粒子，恢复后继续

## 新增一种符号

1. 在 `type Burst` 里加名字
2. 在 `burst()` 的 `anchor` 表里加锚点，必要时加生成数量
3. 在 `drawParticle()` 里加一条绘制分支
4. 在 `CompanionApp.vue` 的触发点调用 `fx.value?.burst('新名字')`

## 表演片段

`perform` 槽位里的两段表演是同一套渲染代码烘焙出来的 PNG 帧：`perform-holo-*.png`（全息终端，90 帧 / 3 秒）和 `perform-jump-*.png`（跃起，54 帧 / 1.8 秒），由 `motions.json` 描述。它们在动作页有独立按钮（全息终端 / 跃起），也会参与待机的随机表演。

重新烘焙用工作区的 `video-demo/fxdemo/render_fx.py` 与 `integration/render_perform.py`，改参数后重跑即可；注意单动作 PNG 上限 120 帧。
