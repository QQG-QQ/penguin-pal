# PenguinPal 0.5 动作视频

0.5.2 仅修正左右交替步态：两脚分别落地和抬起，4 个新关键姿态构成0.8秒完整周期，30 fps/24帧。旧行走素材保留，当前两段使用 gait 文件名；制作、提示词与验证详见 [walk-gait.md](walk-gait.md)。下文0.5.1的20帧行走数据属于旧版。

使用内置 imagegen，根据本项目 companion-v2/idle.png 保持角色身份生成 4 张透明图集。完整最终提示词在 prompts.json；没有调用外部图片 API、AI 视频模型或复制参考项目的动画。

- social.atlas.png：开心、摸摸、戳戳、尾巴。
- life.atlas.png：吃鱼、休息、醒来、扫地。
- expressions.atlas.png：思考、张望、眨眼、说话。
- motion.atlas.png：小脾气、待机、落地、眩晕。

每张图集为 8 列 × 4 行，每行一个动作的 8 个关键帧。保留原图与真实 alpha，按透明分隔线切分并归一化为 256×256。按动作统一缩放、脚底锚点 y=244，没有用代码重绘角色。16 组新动作共 128 帧；左右行走另复用旧版自有 8 帧。

18 个 WebM 加上旧版挥手，合计 19 段独立视频，覆盖面板 14 个动作以及待机、落地、眩晕和左右行走。工作、等待、结束等状态复用相应视频。拖起/下落仍保留原帧图，不声称所有物理状态都是新视频。

0.5.1 使用 RIFE 4.6 离线插帧，从 6 fps 原关键帧制作 30 fps 视频（挥手原输入为 7 fps）。8 帧动作输出 40 帧、约 1.33 秒；4 帧行走输出 20 帧、约 0.67 秒；挥手输出 34 帧、约 1.13 秒。保持 VP9 yuva420p、256×256、无音轨，PNG 回退仍保留原关键帧速度。新增 greeting.webm 用于内置挥手，旧 companion-v2/greeting.webm 和已导入外部包不修改。

0.5.0 只用 fps=24 重复帧，实际动作变化仍约 6 次/秒。0.5.1 使用 RIFE 4.6（官方便携版 20221029），循环动作补首帧作为末尾过渡，单次动作固定末帧。RIFE 输入必须为三通道 RGB；将同一关键帧分别合成在黑/白背景并上下拼接，插值后按两者差值恢复 alpha，并还原颜色；没有用抠绿屏删除角色颜色。补帧是从原画估计中间运动，不能增加原作画的姿态细节，快速遮挡仍可能产生局部变形。FFmpeg minterpolate 两轮试片因脸/手重影被否决，未放入交付程序。

双背景恢复后，将 alpha 小于 4/255 的数值残差置零，避免空白背景出现极淡杂点；其他透明边界保留。每段逐帧检查四角 alpha 为 0、角色有足够不透明区域、可见画面变化和循环接缝；原始 PNG 不修改。

制作工具为本地 FFmpeg 7.1（imageio-ffmpeg 0.6.0）和 [RIFE ncnn Vulkan 官方版](https://github.com/nihui/rife-ncnn-vulkan/releases/tag/20221029)，后者 [MIT 许可证](https://github.com/nihui/rife-ncnn-vulkan/blob/master/LICENSE)。重新制作：`node scripts/prepare-companion-videos.mjs <ffmpeg-executable> <rife-20221029-directory>`。脚本只读取原 PNG，先在本项目临时目录编码成功再替换视频，输出 video-build.json，记录工具与模型哈希。制作需要兼容 Vulkan 的 GPU（当前脚本选设备 0）；成品运行不需要 Vulkan、RIFE、FFmpeg、插帧计算或新增依赖。模型和工具未打入成品，没有上传素材或调用收费 API。

motions.json 是内置媒体描述，URL 在构建时解析；不是直接导入的外部 manifest。外部包保持原 schema，缺失动作由内置包补充，不能将内部 motions.json 当成外部清单导入。
