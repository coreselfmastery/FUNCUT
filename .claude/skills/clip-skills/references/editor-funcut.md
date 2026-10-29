---
name: editor-funcut
description: >
  用 FUNCUT（Concat 引擎）真正动手剪辑。FUNCUT 是本地优先、跨平台的开源剪辑器（Rust + Slint），
  自带 MCP 服务器（`concat --mcp`）与 JSON-RPC API，Agent 可经 MCP 直接建工程、切分/裁剪/排轨、
  打关键帧、变速、加字幕/动效合成、调色、混音、导出，并自带"眼睛"（preview_frame / watch_render
  / craft_critique）与耳朵（transcribe / measure_loudness / emotion）。当目标软件是 FUNCUT、
  需要把剪辑决策落地为真实时间线操作时用本技能。建议在独立子 Agent 中加载，避免与其他软件上下文混淆。
---

# FUNCUT 执行技能

把 `clip-skills` 知识内核（`references/01..15`）的剪辑决策，落地为 FUNCUT 时间线上的真实操作。本技能负责"怎么动手"，不负责"怎么判断"；判断依据永远先读内核分册，再回这里找对应工具。

> 红线同内核：**不臆造工具能力**。每个工具的参数以运行时 `tools/list` 返回的 schema 为准（本册的工具名与语义基于 FUNCUT 的 `funcut-director` 技能 `prompts/tool_reference.md` 核对过）；`effects_list` 给出每个效果包的实时参数表——**能读就不要写死**。

## 一、先决条件 & 连接

- **环境**：FUNCUT（应用名 Concat）是 Rust + Slint 桌面应用，本地优先（素材不出机器）。构建从仓库 `src/` 目录 `cargo run --profile quick -p concat`（注意 Cargo workspace 根在 `src/`，不在仓库根）。
- **MCP 连接**：编辑器自带 MCP 服务器。窗口开着时 `concat --mcp window`（Settings > Automation > Scripts and apps），无窗口 `--mcp headless`，`--mcp auto` 自选。Claude Code 类工具可直接挂；Forge 工作台读仓库 `.forge/mcp.toml` 自动接上。
- **长活是 job**：转写、分析、TTS、模型下载、导出都返回 `{"job":"jN"}`，用 `analysis_wait`/`export_wait`（默认 60s、上限 600s，没完就再等一次）取结果。
- **一次一个 API 会话**：一个文件夹一个会话；窗口开着的工程不能被 headless 再开（反之亦然）。

## 二、知识内核 → FUNCUT 工具映射（总表）

| 内核分册（决策） | FUNCUT 落地（工具） |
|:---|:---|
| `01` 视频类型/骨架 | `project_set_video` 定画幅 → 计划分节（hook/intro/chapters/cta），节奏地板见 `05` |
| `02` 音频驱动切点 | `transcribe`（带逐词时间戳！`words` 是唯一可信词时）→ `analyze_silence`（气口）→ `clip_cut_spans`（删死气，见下方"80ms 留白规则"） |
| `03` A/B-roll 匹配替换 | `openvid_search` / `openvid_search_batch`（语义搜 B-roll）→ `stock_contact_sheet`（**先看图再选**）→ `openvid_fetch` + `openvid_import` → `clips_add_sequence` / `clip_add`，B-roll 整轨 `clip_audio_levels_db {-60}` 静音 |
| `04` 声音与音乐 | `bgm_search/fetch/import`（铺底）→ `analyze_beats`（节拍时间戳）→ `sounds_search/add`（音效峰值对切点，`syncAt` 给出峰位）→ `clip_audio_levels_db` 的 keyframes 做 duck（人声让位） |
| `05` 节奏与结构 | `suggest_cuts`（无 Python 时的 BPM+对拍切点）→ 计划的 cuts/分钟地板；波峰/掉粉点用 `watch_render` 事后核查 |
| `06` 调色 | `effects_list {kind:"filter"}` 选包 → **一个 `effect_layer_add {start:0, duration:整片时长}`**（duration 必写！缺省只调 5 秒）→ `effect_key_set` 调包自带参数 → `preview_frame` 在 25%/75% 处 A/B 验证 |
| `07` 关键帧与运动 | `keyframe_set`（`scale/offsetX/offsetY/rotation/opacity/volume` + 蒙版字段；`at` 是片段 0–1 分数；`ease` 是 CSS cubic-bezier `[x1,y1,x2,y2]`）+ `clip_transform`；"后期拉近"= scale+offsetX/Y 同打（内核铁律直接适用） |
| `08` 变速与帧率 | `clip_speed`（0.0625–16 匀速）→ 曲线变速 `edit_apply` 发 `setClipSpeedCurve`（缓动变速曲线）→ 定格 `freezeFrame`；**无光流补帧**（见"能力边界"） |
| `09` 抠像/蒙版/合成 | 色度 `concat.chroma-key`（参数 color/similarity/softness/spill）或 `concat.luma-key`；形状蒙版 `clip_mask`（linear/mirror/circle/rectangle/heart/star + feather/roundness，可用 `keyframe_set` 动画）；混合模式与多层合成走 `motion` 合成系统 |
| `10` 跟踪与动效包装 | **无平面跟踪**（见边界）；文字包装走 `title_add`（31 预设）/ `motion_*` 合成系统（12 预设 comp，`motion_overlay` 叠透明层）；已知运动用 `keyframe_set` 逐段贴合 |
| `11` 多机位 | **无原生 multicam**：多机位 = `track_add` 多轨 + 按音频对齐（`transcribe`/`analyze_silence` 找共同峰）→ "导播切点+机位"清单落到对应轨的 `clips_split`/`clips_remove`，或切段后 `clips_add_sequence` 排主轨 |
| `12` 导出与交付 | `export_start {crf 20, preset "medium", codec h264/hevc/av1, width/height/rate}` → `export_wait` → **`measure_loudness`** 实测 LUFS（短视频 −14、广播 −23）→ `watch_render {path}` 看成片文件 |
| `13` 字幕与文字 | 段落字幕 `captions_add {start,end,text}`；逐词 kinetic `captions_animate`（**只吃实测词时**，transcribe 的 `words`）；标题 `title_add`（样式见其预设表） |
| `14` 转场决策 | `clip_update {patch:{transitionIn:{id,duration}}}` 打在**进场片段**上；22 个转场包 `effects_list {kind:"transition"}` 实查（dissolve/fade-black/fade-white/whip-pan/push/slide/spin/cube/iris/clock-wipe/linear-wipe/shape-wipe/swirl/zoom-transition/zoom-blur-transition/blur-dissolve/film-burn/glitch-transition/glitch-warp/light-leak-flash/light-leak-transition/rgb-split-wipe） |
| `15` 人声与对白 | `concat.denoise`（降噪）/ `concat.enhance-voice` / `concat.bass`/`concat.treble`/`concat.pitch` 等 25 个音频包（`effects_list {kind:"audio"}`）→ `concat.loudness` 或 `measure_loudness` 达标；补录/配音 `tts_generate`（delivery/speed/pauses 控语气） |

## 三、标准流水线在 FUNCUT 的落地（对接 orchestrator）

输入＝orchestrator 传来的 `audio_timeline` + B-roll 匹配映射表 + 画幅/时长目标。

1. **建工程对画幅**：`project_create {location,name}` → `project_set_video`（横 1920×1080 / 竖 1080×1920，与计划 `aspect` 一致；不一致**先设画幅再放素材**）。
2. **导入与探查**：`media_import` 主素材；**任何不熟的文件先 `media_probe`**（时长/分辨率/帧率/编码）再切。
3. **精剪声骨**：`analyze_silence` → **80ms 留白规则**（工具不替你做：每个静音 span 两端各收 ~80ms 再交给 `clip_cut_spans`，句尾气口才自然）→ `clip_cut_spans {spans, ripple:true}` 一次删净死气。
4. **切点重对齐**：transcribe 的时间是**源文件相对时间**——ripple 删减后，所有后续 segment 边界要按已删 span 精确平移再用（词时只信 transcribe 返回的 `words`，不做估算插值）。
5. **贴 B-roll**：按匹配映射表 `clips_add_sequence` / `clip_add` 上层轨；每条 `clip_frame {framing:"fill"}` 盖满画布（竖屏用 OpenVid 横版素材 fill 裁切，别搜 vertical——库里没有）；B-roll 轨 `clip_audio_levels_db {baseDb:-60}` 整轨静音；入出场 `keyframe_set` / `clip_update {fadeIn/fadeOut}`（**raw in raw out 是缺陷**）。
6. **声音层**：`bgm_import` 铺底 → `analyze_beats` 得节拍 → 切换落节拍、段落落强拍；`clip_audio_levels_db` keyframes 做 duck（人声下压 −12dB 档，紧 −9 / 松 −15）；`sounds_add` 音效峰对切点（`syncAt`）。
7. **文字与包装**：`captions_add` 段落字幕 / `captions_animate` 逐词 kinetic / `title_add` 标题 / `motion_bake`+`motion_overlay` 合成层（细看 FUNCUT 自带 `motion` 技能，那才是 comp 的完整语法）。
8. **调色**：整片一个 `effect_layer_add`（duration 写全片！）；强度 `effect_key_set` 用包自己的键（`concat.warm` 是 `temperature` 开尔文，`concat.teal-orange` 是 `amount` 0–100——**每个包的键从 `effects_list` 读**）。
9. **验证闭环**（FUNCUT 独有优势，务必走完）：`watch_render` 整片一张标注接触表 + 实测工艺事实（黑帧/黑边/重复镜头/音包络）→ `craft_critique` 六项 0–10 打分 → **至少改一处再 watch_render 一次**才许导出。
10. **导出**：`export_start` → `export_wait` 确认文件落盘 → `watch_render {path, audio:true}` + `measure_loudness {path, targetLufs}` 验证**导出文件本身**；响度不达标用 `clip_audio_levels_db` 修后重测（**计划写了目标不等于达标，必须实测**）。

## 四、关键能力细节（易错点）

### 4.1 批次与撤销
- 每个 wrapper 工具= 一条可撤销 `edit.apply` 命令；多片段改动用 `edit_apply {commands:[…]}` 打成**一个撤销步**。出错 `edit_undo` 回退，别逐个手修。
- 编辑**即存**（每条 apply 后工程已写盘），不需要频繁 `project_save`，但收尾仍建议保存。

### 4.2 关键帧（对接 `07`）
- `keyframe_set` 的 `at` 是 **0–1 片段分数**，不是秒不是帧——换算先做。
- `ease` 是 CSS cubic-bezier 四元数组；内核的"缓入缓出"= `[0.16,1,0.3,1]` 这类；缓动曲线跨参数必须同套（内核铁律二）。
- "后期拉近"在 FUNCUT 的落地：`scale` 与 `offsetX/offsetY` **同 at 值打帧、同 ease**，只打 scale 就是原地放大（内核经典坑直接适用）。
- `volume` 键是线性增益（1=不变）不是 dB；要 dB 精度的 duck 用 `clip_audio_levels_db {keyframes:[{at,gainDb,ease}]}`。

### 4.3 转场（对接 `14`）
- **永远是 `clip_update {patch:{transitionIn:{id,duration}}}` 打在进场片段上**；`effect_layer_add` 放转场**渲染不出东西**（层带不了双输入转场）。
- 进场片段必须与前一片段**同轨相邻**（半帧内），移动/有缝的切点上转场会静默失效。
- 密度：段落切换处必一个；普通切点**直切优先**（内核原则：转场是调味料）。

### 4.4 抠像与蒙版（对接 `09`）
- `concat.chroma-key` 参数对照内核"色度抠像"：`similarity`≈扣除范围、`softness`≈边缘羽化、`spill`≈去溢色（去绿边）；键出色用 `color`。
- 抠完必检：FUNCUT 的检查方式是 `preview_frame` 在入/中/出三处看边缘（等价于内核"预览轴划查"）。
- "抠像三明治"层级：主轨放背景、上层轨放抠出主体、再上层文字/包装——`track_add` 开轨即可自由排层。

### 4.5 字幕（对接 `13`）
- `captions_add` 只做段落级 `{start,end,text}`，**不做词对齐**；逐词 kinetic 必须 `captions_animate` 且只吃 transcribe 的实测 `words`。
- 转写模型选择：默认 `base.en` 只支持英文，非英文选多语言模型（`small`）；先 `models_list` 后 `models_download`，**不要循环重试"not downloaded"**。
- 字幕安全区/字号：`title_add` 的 `fontSize` 是**帧高分数**（0.04 默认，短视频 0.05–0.06）；`offsetY 0.35`≈下三分之一。字幕别贴边、别盖脸。

### 4.6 变速（对接 `08`）
- 匀速 `clip_speed 0.0625–16`（改的是时间线上长度，源 stretch 保留）。
- 曲线变速（V 谷反差/平顶山）→ `edit_apply` 发原生 `setClipSpeedCurve` 命令；定格 `freezeFrame`。
- **慢放卡顿警告同样适用**：FUNCUT 无光流补帧，源帧率不够的慢放会卡——按内核 `08` 的前置判断，要么建议重拍高帧率，要么接受帧融合级的观感（导出到达芬奇/Topaz 做专业补帧）。

## 五、能力边界（诚实清单，不臆造）

| 内核需求 | FUNCUT 现状 | 替代路径 |
|:---|:---|:---|
| 光流补帧 | 无 `minterpolate` 级补帧 | 高帧率重拍；或导出到达芬奇/专业补帧工具 |
| 平面跟踪（透视贴合） | 无 | 已知运动用 `keyframe_set` 手动逐段贴合；复杂跟踪导出 After Effects(Mocha)/达芬奇 Fusion |
| 智能抠像（无绿幕 AI 主体） | 无 AI 人物分割 | 绿幕用 `concat.chroma-key`；亮度用 `concat.luma-key`；无绿幕人物抠像导出到达芬奇 Magic Mask / Premiere |
| 原生多机位导播台 | 无 multicam UI | 多轨 + 音频对齐 + "切点+机位"清单批落地（§二 `11` 行） |
| LUT（.cube） | 无 `lut3d` 等价物 | 用 `concat.curves`/`concat.color-wheels`/风格滤镜包（teal-orange/noir/vintage…57 个 filter）表达同一调色决策 |
| 音频互相关自动合板 | 无 | 手动对齐：各机位 `analyze_silence`/`transcribe` 找同一拍手/起句点算偏移，`clips_move` 对齐 |

> 以上任何一项，若用户坚持在 FUNCUT 内完成且无替代路径——**明说做不到**，给导出方案，不硬做。

## 六、黑场红线在 FUNCUT

- 流水线遗留的黑场/未覆盖硬切：**不自动删主轨**——`timeline_read` 找缝、`collision_check` 查叠，把位置清单交回用户裁。
- 大批量切分/替换前：把将执行的 `edit_apply` 命令清单先报给 orchestrator/用户，留叫停机会；真出事 `edit_undo` 整批回退（这就是"关键替换可回溯"在 FUNCUT 的实现）。
- 耗算力操作（变速/稳定 `clip_stabilize`/复杂合成）放粗剪定稿后；做完先 `export_start` 出一版备份再继续。

## 七、验证闭环（内核"好坏怎么判"的落地）

FUNCUT 比其他 NLE 多一层**实测**能力，内核各册的判断应尽量用它背书：

| 内核判断 | 实测工具 |
|:---|:---|
| 节奏/时长/切率（`05`） | `watch_render` 接触表 + `suggest_cuts` 的 BPM/切点 |
| 调色前后（`06`） | `preview_frame` 25%/75% 两点 A/B |
| 动效衔接（`07`） | `preview_frame` 三时刻；`motion_frame`（合成层单帧，内联图） |
| 响度/duck 达标（`12`/`15`） | `measure_loudness`（EBU R128 LUFS/TP，含与目标的误差） |
| 整片六维打分 | `craft_critique`（hook/story/rhythm/shotChoice/design/sound，0–10） |
| 语音是否完整 | 重转写导出文件比对词时（matchRate） |

## 八、与 FUNCUT 自带技能的关系（重要）

FUNCUT 仓库自带一套技能，本技能落地时要**让位与协作**，不要抢戏：

- **`funcut-director`** 是 FUNCUT 内的**总执行管线**（0–8 阶段、硬地板、验证闭环）。在 FUNCUT 内自主执行时，**以它为主干**；clip-skills 内核作为"怎么剪"的工艺判断层喂给它（它的 §7 有专门路由表）。
- **`motion`** 是 FUNCUT 的动效合成引擎（comp/预设/bake/overlay）。一切"设计过的图形"（标题/人名条/数据卡/kinetic 字幕）走它，不走本册。
- **`maestro`** 是设计/动效/影像审美判断层。分工：maestro 管"好看不好看"（字体/色彩/层级/动效气质），clip-skills 管"剪得对不对"（切点/B-roll/节奏/调色流程/导出参数）。
- 冲突时：FUNCUT director 的硬地板 > maestro/clip-skills 的参考判断（与 maestro 在 FUNCUT 内的既有规则一致）。

> 在 FUNCUT **之外**（用户只是问怎么剪、或用别的软件），本册不用——回 SKILL.md 路由到对应 editor 或直接答疑。

## 九、查找指引（信息不足时去查什么）

- 工具参数以运行时 `tools/list` 的 schema 为最终依据（本册只保证工具名与语义已核对）。
- 效果/滤镜/转场/音频包的实时清单与参数：`effects_list {kind}`。
- 深入文档：FUNCUT 仓库 `docs/`（MCP.md 工具总览、MOTION.md 动效引擎、EMOTION.md 情绪读、forge/FORGE.md 技能安装）。
- 技能协作细节：`.claude/skills/funcut-director/SKILL.md`（管线与地板）、`.claude/skills/motion/SKILL.md`（comp 语法）、`docs/CLIPSKILLS.md`（本技能在 FUNCUT 的定位与更新）。
