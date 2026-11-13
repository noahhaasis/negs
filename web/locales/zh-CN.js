// The Simplified Chinese interface strings. Keys are the English sources
// (web/i18n.mjs's tr() looks them up, showing the English when one is
// missing); web/checks/i18n.test.mjs checks that every key is still in the
// sources, that every shown string has a key, and that {name} placeholders
// match one for one.
window.NEGS_I18N.dict = {

  // ---- 页面与空状态（index.html）----------------------------------------------

  "Negs — spektrafilm web app: free online film simulation": "Negs-将你的照片，盛于胶片之上：免费在线胶片模拟",
  "Back to the Negs website": "回到 Negs",
  "Drop to open": "打开照片",
  "Your photo, through film": "将你的照片，盛于胶片之上",
  "Negs simulates exposing film, developing it, and making a print. This quick tour shows how each stage shapes your photo. You can skip it or leave at any time.":
    "Negs 完整模拟胶片摄影的三个环节：曝光、显影、放印，这段简短教程将展示如何把你的照片变成胶片",
  "Start tutorial": "开始教程",
  "Not now": "暂不",
  "Open a photograph": "打开照片",
  "A camera RAW, JPEG, PNG or TIFF. Drop it here, or choose one.": "拖到这里，或点击选择本地文件。支持相机 RAW、JPEG、PNG、TIFF",
  "Your photo stays in your browser.": "照片将在这里显示",
  "Choose photo…": "选择照片…",
  "Try a sample photo": "试试示例照片",

  // ---- 主按钮与工具 --------------------------------------------------------------

  "Open…": "打开…",
  "Export": "导出",
  "More": "更多",
  "Presets and LUT": "预设与 LUT",
  "Import preset…": "导入预设…",
  "Export preset…": "导出预设…",
  "Export LUT…": "导出 LUT…",
  "A 33 × 33 × 33 CUBE file, sRGB in and out, with the current film and paper color settings. Excludes auto exposure, print filter shifts, preflash, grain, halation, glare, diffusion, blur, sharpening, RAW development and Post edits.":
    "33 × 33 × 33 的 CUBE 文件，sRGB 输入输出，包含当前胶片与相纸的色彩设置。不包含自动曝光、放印滤镜偏移、预闪、颗粒、光晕、眩光、柔光、模糊、锐化、RAW 显影以及后期编辑",
  "Cancel": "取消",
  "Done": "完成",
  "Crop": "裁剪",
  "Crop and straighten (C or ⌘/Ctrl+K)": "裁剪与校正（C 或 ⌘/Ctrl+K）",
  "Rotate counterclockwise": "逆时针旋转",
  "Rotate clockwise": "顺时针旋转",
  "Turn left (Shift+R)": "向左旋转（Shift+R）",
  "Turn right (R)": "向右旋转（R）",
  "Show Original": "显示原图",
  "Hold to show the original (\\)": "按住查看原图（\\）",
  "Tap to show the original; tap again to go back": "点击查看原图，再次点击返回",
  "Fit": "适应",
  "Fit the photograph in the view (double-click)": "适应视图（双击）",
  "Actual size: a pixel of the photograph on each screen pixel (double-click)": "实际大小（双击）",
  "Drag crop corners or edges to resize; drag inside to move": "拖动四角或边缘调整大小；拖动中间移动位置",
  "Next sample →": "下一张示例 →",
  "Sample photo": "示例照片",
  "Sample photos": "示例照片",
  "Only Sample 1 is available offline once saved.": "保存后，仅示例 1 可离线使用",
  "Credits & source": "致谢与源码",

  // ---- 侧栏：胶片与相纸 ----------------------------------------------------------

  "Prep": "预处理",
  "Lab": "暗房",
  "Output": "输出",
  "Scan": "扫描",
  // The phone's sheet tab for the pickers; not 胶片, which is the Film
  // picker's own label and the Lab's Film section.
  "Stock": "胶片与相纸",
  "Inspector": "检查器",
  "Film": "胶片",
  "Development": "显影",
  "Print development": "相纸显影",
  "Scan film directly": "直接扫描胶片",
  "Skip printing and scan the developed film, as for slides.": "跳过放印，直接扫描显影后的胶片",
  "Paper": "相纸",
  "Profile default": "默认",
  "{n} min": "{n} 分钟",
  "Favorites": "收藏",
  "Search films": "搜索胶片",
  "No {what} match “{query}”": "没有找到与“{query}”匹配的{what}",
  "films": "胶片",
  "papers": "相纸",
  "Add to favorites": "添加到收藏",
  "Remove from favorites": "取消收藏",
  "Add {name} to favorites": "将{name}添加到收藏",
  "Remove {name} from favorites": "将{name}移出收藏",
  "Color Negative": "彩色负片",
  "Color Slide": "彩色反转片",
  "Motion Picture": "电影胶片",
  "Color Paper": "彩色相纸",
  "Motion Picture Print": "电影印片纸",
  "Black & White": "黑白",

  // ---- 预处理（Prep）--------------------------------------------------------------

  "RAW White Balance": "RAW 白平衡",
  "White balance": "白平衡",
  "Temperature (K)": "色温 (K)",
  "Tint": "色调",
  "Brightness": "亮度",
  "Match camera brightness": "匹配相机亮度",
  "Develops the RAW about as bright as the camera’s own preview, and keeps it there when you change film, paper or a preset. Exposure adds to it.": "显影 RAW 时，亮度大致相当于相机自身的预览，更换胶片、相纸或预设后仍保持这一亮度。曝光调整会在此基础上叠加",
  "Exposure (EV)": "曝光 (EV)",
  "Camera brightness": "相机亮度",
  "Auto exposure": "自动曝光",
  "Metering": "测光",
  "Tone": "影调",
  "Highlights": "高光",
  "Shadows": "阴影",
  "Contrast": "对比度",
  "Lift": "提升",
  "Gain": "增益",
  "Color": "色彩",
  "Color Boost": "色彩增强",
  "Saturation": "饱和度",
  "Detail": "细节",
  "Midtone Detail": "中间调细节",

  // ---- 暗房（Lab）：相机与胶片 ----------------------------------------------------

  "Camera": "相机",
  "Negative exposure (EV)": "负片曝光 (EV)",
  "Film format (mm)": "胶片规格 (mm)",
  "Lens blur (µm)": "镜头模糊 (µm)",
  "Lens Diffusion": "镜头柔光",
  "Filter": "滤镜",
  "Strength": "强度",
  "Spatial scale": "空间范围",
  "Halo warmth": "光晕暖度",
  "Core intensity": "核心强度",
  "Core size": "核心大小",
  "Halo intensity": "光晕强度",
  "Halo size": "光晕大小",
  "Bloom intensity": "泛光强度",
  "Bloom size": "泛光大小",
  "Film Curves": "胶片曲线",
  "Gamma": "伽马",
  "Fast gamma": "高感光层伽马",
  "Slow gamma": "低感光层伽马",
  "Red gamma": "红色伽马",
  "Green gamma": "绿色伽马",
  "Blue gamma": "蓝色伽马",
  "Developer exhaustion": "显影液耗尽",
  "Grain": "颗粒",
  "Red granularity": "红层颗粒度",
  "Green granularity": "绿层颗粒度",
  "Blue granularity": "蓝层颗粒度",
  "Granularity": "颗粒度",
  "Red uniformity": "红层均匀度",
  "Green uniformity": "绿层均匀度",
  "Blue uniformity": "蓝层均匀度",
  "Uniformity": "均匀度",
  "Coarse grain size": "粗颗粒大小",
  "Medium grain size": "中颗粒大小",
  "Fine grain size": "细颗粒大小",
  "Dye clouds (µm)": "染料云 (µm)",
  "Blur (px)": "模糊 (px)",
  "Sharpen amount": "锐化强度",
  "Sharpen radius (px)": "锐化半径 (px)",
  "Clump blur (µm)": "团簇模糊 (µm)",
  "Clump size (nm)": "团簇大小 (nm)",
  "Halation": "光晕",
  "Amount": "数量",
  "Scatter amount": "散射强度",
  "Scatter scale": "散射范围",
  "Bounces": "反射次数",
  "Bounce decay": "反射衰减",
  "DIR Couplers": "DIR 成色剂",
  "Diffusion (µm)": "扩散 (µm)",
  "Tail (µm)": "拖尾 (µm)",
  "Tail weight": "拖尾权重",
  "Film Base": "片基",
  "Density": "密度",
  "Tilt": "倾斜",
  "Cyan": "青色",
  "Magenta": "品红",
  "Yellow": "黄色",

  // ---- 暗房（Lab）：放印与扫描 ----------------------------------------------------

  "Print": "放印",
  "Printing is skipped while scanning film directly.": "直接扫描胶片时跳过放印",
  "Enlarger": "放大机",
  "Print exposure": "放印曝光",
  "Magenta shift": "品红色校色",
  "Yellow shift": "黄色校色",
  "Balance Print": "放印平衡",
  "Balancing…": "平衡中…",
  "Sets the magenta and yellow shifts so a mid-grey prints neutral with the current film, bases, paper and enlarger. Use it after changing the film base.":
    "设置品红色与黄色校色，使中灰在当前胶片、片基、相纸和放大机下放印为中性。更改片基后请重新执行",
  "Exposure compensation": "曝光补偿",
  "Preflash exposure": "预闪曝光",
  "Preflash magenta": "预闪品红",
  "Preflash yellow": "预闪黄",
  "Print Diffusion": "放印柔光",
  "Print Curves": "相纸曲线",
  "Print Base": "纸基",
  "Glare": "眩光",
  "Roughness": "粗糙度",
  "Scan & Color": "扫描与色彩",
  "Scanner": "扫描仪",
  "Lens blur (px)": "镜头模糊 (px)",
  "Color noise reduction": "彩色噪点抑制",
  "Unsharp radius (px)": "锐化半径 (px)",
  "Unsharp amount": "锐化强度",
  "White correction": "白场校正",
  "White level": "白场电平",
  "Black correction": "黑场校正",
  "Black level": "黑场电平",
  "Color Management": "色彩管理",
  "Output gamut": "输出色域",
  "Input gamut": "输入色域",
  "Spectral upsampling": "光谱上采样",
  "Off": "关闭",
  "As Shot": "拍摄时设置",
  "Daylight": "日光",
  "Tungsten (2850 K)": "钨丝灯 (2850 K)",
  "Custom": "自定义",
  "Average": "平均测光",
  "Median": "中值测光",
  "Center-Weighted": "中央重点测光",
  "Partial": "局部测光",
  "Matrix": "矩阵测光",
  "Multi-Zone": "多区测光",
  "Highlight-Weighted": "高光重点测光",
  "xy Chromaticity": "xy 色度",

  // ---- 输出（Output）：边框 ---------------------------------------------------------

  "Border": "边框",
  "Add border": "添加边框",
  "Style": "样式",
  "Solid": "纯色",
  "Keyline": "细线框",
  "Width": "宽度",
  "Line width": "线条宽度",
  "Color": "颜色",
  "Line color": "线条颜色",
  "Extend to 3:4 for Instagram": "扩展为 3:4（Instagram）",
  "Include in export": "包含在导出中",

  // ---- 裁剪 -----------------------------------------------------------------------

  "Aspect Ratio": "宽高比",
  "Crop aspect ratio": "裁剪宽高比",
  "Height": "高度",
  "Drag to resize, or tap": "拖动调整大小，或点击",
  "Original": "原比例",
  "Freeform": "自由",
  "Square · 6×6": "正方形 · 6×6",
  "Custom ratio width": "自定义比例宽度",
  "Custom ratio height": "自定义比例高度",
  "Crop orientation": "裁剪方向",
  "Landscape": "横向",
  "Portrait": "纵向",
  "Straighten": "校正",
  "Level a crooked horizon. The crop shrinks to stay on the photograph.": "校正倾斜的地平线。裁剪框会缩小以保持在照片内",
  "Reset angle": "重置角度",
  "Straightening angle in degrees": "校正角度（度）",
  "Straighten photograph": "校正照片",
  "Drag to level the horizon; double-click to reset": "拖动以校正水平，双击重置",
  "Size": "尺寸",
  "Reset Crop": "重置裁剪",
  "Drag corners or edges to resize, or drag inside to move. Drag the dial below the photograph to level it. Return applies; Escape cancels. Borders are added after cropping.":
    "拖动角或边调整大小，拖动中间移动。拖动照片下方的拨盘校正水平。回车应用，Esc 取消。边框在裁剪之后添加",

  // ---- 设置对话框 ------------------------------------------------------------------

  "Settings": "设置",
  "Advanced controls": "高级控件",
  "Show every camera, film, print and scan setting. Off, the Lab shows the ones most photos need, plus any that you or a preset changed.":
    "显示全部相机、胶片、放印和扫描设置。关闭时，暗房只显示大多数照片需要的设置，加上你或预设改动过的。",
  "Film and paper previews": "胶片与相纸预览",
  "Show each film and paper on your photograph. They use about 100 MB, and turn off if Negs runs out of memory.":
    "在你的照片上预览每种胶片和相纸。大约占用 100 MB，内存不足时会自动关闭。",
  "Fotufilm films": "Fotufilm 胶片",
  "All": "全部",
  "From datasheets": "仅限数据表",
  "None": "不显示",
  "Films ported from Fotufilm. From datasheets hides the eight it models from sample photographs rather than the manufacturer’s data. Photos already using a hidden film keep it.":
    "从 Fotufilm 移植的胶片。“仅限数据表”会隐藏其中八款根据样片而非厂商数据建模的胶片。已在使用被隐藏胶片的照片会保留它。",
  "Match new RAWs to camera brightness": "新 RAW 匹配相机亮度",
  "Start each RAW you open as bright as the camera showed it (Prep › Match camera brightness).":
    "打开的每张 RAW 都以相机呈现的亮度起步（预处理 › 匹配相机亮度）。",
  "Engine": "引擎",
  "WebGPU uses the graphics chip and is much faster. CPU works without it.": "WebGPU 使用显卡，速度快得多。CPU 无需显卡也可运行。",
  "Language": "语言",
  "The interface language. Negs reloads to apply it.": "界面语言。切换后重新加载 Negs。",

  // ---- 预设与链接 ------------------------------------------------------------------

  "Export preset": "导出预设",
  "Name": "名称",
  "Preset link": "预设链接",
  "Copy link": "复制链接",
  "Includes Lab settings only. Your photo, Prep adjustments, crop and border are not included.":
    "只包含暗房设置。不包含照片、预处理调整、裁剪和边框",
  "Close": "关闭",
  "Download preset": "下载预设",
  "Shared preset": "收到的预设",
  "Try on a sample photo": "在示例照片上试用",
  "No photograph is included in this link.": "此链接不包含照片",
  "Continue to the editor": "继续进入编辑器",
  "Enter a name.": "输入名称",
  "Use at most {n} characters.": "最多使用 {n} 个字符。",
  "A name can’t contain / \\ : * ? \" < > or |.": "名称不能包含 / \\ : * ? \" < > 或 |。",
  "A name can’t start with a period.": "名称不能以句点开头。",
  "A name can’t start with a space or end with a space or period.": "名称不能以空格开头，也不能以空格或句点结尾",
  "“{name}” is reserved on Windows.": "“{name}”是 Windows 的保留名称",
  "The preset in this link is too large.": "链接中的预设过大。",
  "This preset link is damaged or incomplete.": "预设链接已损坏或不完整",
  "This browser can’t make preset links. Download the preset instead.": "此浏览器无法生成预设链接，请改为下载预设",
  "This preset is too large for a link. Download the preset to share it.": "预设过大，无法生成链接。请下载预设来分享",
  "This preset link is empty.": "预设链接为空",
  "This preset link is too long.": "预设链接过长",
  "This preset link is from a newer Negs. Reload to update, then open it again.": "此预设链接来自更新版本的 Negs。请重新加载以更新，然后再次打开",
  "This browser can’t open preset links.": "此浏览器无法打开预设链接",
  "Applied “{name}”." : "已应用“{name}”",
  "Applied “{name}”, but its RAW white balance couldn’t be applied: {message}. The white balance is back to the photograph’s.":
    "已应用“{name}”，但其 RAW 白平衡未能应用：{message}。白平衡已恢复为照片原本的设置",
  "Couldn’t apply “{name}”: {message}": "无法应用“{name}”：{message}",
  "Couldn’t apply “{name}”: the file is too large to be a preset.": "无法应用“{name}”：文件太大，不是预设文件",
  "Couldn’t read “{file}”: {message}": "无法读取“{file}”：{message}",
  "Exporting LUT…": "正在导出 LUT…",
  "Exported {filename}.": "已导出 {filename}",
  "Couldn’t export the LUT: {message}": "无法导出 LUT：{message}",
  "Preparing link…": "正在生成链接…",
  "Enter a valid name for a link.": "请输入有效的名称后再生成链接",
  "Link copied.": "链接已复制",
  "Select the link and copy it.": "请选中链接并手动复制",
  "Couldn’t make a link: {message}": "无法生成链接：{message}",
  "Couldn’t export the preset: {message}": "无法导出预设：{message}",
  "Reading the preset…": "正在读取预设…",
  "This preset can’t be used: {message}": "无法使用此预设：{message}",
  "{film} · scanned directly": "{film} · 直接扫描",
  "Couldn’t save {what}: {message}": "无法保存{what}：{message}",
  "the preset": "预设",

  // ---- 教程 -----------------------------------------------------------------------

  "Tutorial navigation": "教程导航",
  "Tutorial · {index} of {total}": "教程 · 第 {index} 步，共 {total} 步",
  "Step {index} of {total}: {title}": "第 {index} 步，共 {total} 步：{title}",
  "Loading sample…": "正在加载示例…",
  "Returning to the editor…": "正在返回编辑器…",
  "Something went wrong": "出了点问题",
  "Uses a sample photo. About 2 minutes, or take your time. Try the controls as you go. Each step starts fresh.":
    "将使用一张示例照片。大约 2 分钟，也可以慢慢来。过程中随时可以试用控件，每一步都从头开始。",
  "Restoring your photo, edits and layout.": "正在恢复你的照片、编辑和布局。",
  "Next": "下一步",
  "Back": "上一步",
  "Finish": "完成",
  "End tour": "结束教程",
  "Reset step": "重置此步",
  "Retry": "重试",
  "Couldn't restore the editor: {message}": "无法恢复编辑器：{message}",
  "Take a quick tour?": "快速过一遍教程？",
  "The tutorial uses a sample photo and default settings. Your current photo and edits will be restored when you leave.":
    "教程会使用示例照片和默认设置。离开时会恢复你当前的照片和编辑。",
  "Try the controls as you go. Each step starts fresh.": "过程中随时可以试用控件。每一步都从头开始。",
  "Finish or cancel your crop before starting the tutorial.": "开始教程前，请先完成或取消裁剪。",
  "Finish the current operation before starting the tutorial.": "开始教程前，请先完成当前操作。",
  "The sample couldn't be rendered. Try again or return to the editor.": "示例照片渲染失败。请重试，或返回编辑器。",
  "The photo took too long to develop. Try again or return to the editor.": "照片显影耗时过长。请重试，或返回编辑器。",
  "Couldn't open the sample. Check your connection, or save Sample 1 for offline use.": "无法打开示例照片。请检查网络连接，或先把示例 1 保存供离线使用。",
  "Couldn't reopen your photograph. Please retry.": "无法重新打开你的照片，请重试。",
  "You’re ready. Film and paper shape the look; Prep’s Exposure sets the brightness. Reopen Tutorial whenever you like.":
    "你已经上手了。胶片与相纸决定风格，预处理的曝光决定亮度。随时可以重新打开教程。",

  "Choose your film": "选择你的胶片",
  "Each film responds differently to light and colour. Your choice shapes the photo’s colours, contrast, and grain. Start here to find a character you like—then use the other controls to fine-tune it.":
    "每种胶片对光线和色彩的反应都不同。你的选择决定了照片的色彩、对比度和颗粒。从这里开始，找到你喜欢的个性，再用其他控件细细调整。",
  "Choose your print": "选择你的相纸",
  "In a darkroom, light passes through the negative onto photographic paper. The paper adds its own colour and contrast, changing the final photo. Try a few papers with the same film—the difference can be surprisingly large.":
    "在暗房里，光线穿过负片投到相纸上。相纸会加入自己的色彩和对比度，改变最终照片。用同一卷胶片多试几张相纸——差异可能大得出奇。",
  "Start from the right brightness": "从正确的亮度开始",
  "For RAWs, Match camera brightness starts the photo about as bright as your camera showed it, and keeps it there when you change film or paper. With it off, Auto exposure meters the photo like a camera: Center-weighted favours the middle; Highlight-weighted favours bright areas and usually gives a darker result.":
    "对 RAW 照片，“匹配相机亮度”会让照片以接近相机预览的亮度起步，并在你更换胶片或相纸时保持。关闭后，自动曝光会像相机一样测光：中央重点测光偏向中间调，高光重点测光偏向亮部，结果通常更暗。",
  "Match is on for every new RAW, which hides the meter. Here it starts off: turn it on to compare.":
    "每张新 RAW 默认开启匹配，因此测光被隐藏。这里默认关闭：打开它对比一下。",
  "Make it brighter or darker": "调亮或调暗",
  "Prep adjusts your photo before the film sees it. Raise Exposure to brighten the photo; lower it to darken it. Tone, Color and Detail, further down in Prep, work the same way.":
    "预处理在胶片“看到”照片之前先调整它。调高曝光使照片变亮，调低变暗。预处理下方的影调、色彩和细节也是同样的用法。",
  "Exposure adds to the starting brightness, so your change stays when you switch film or paper.":
    "曝光叠加在起始亮度之上，因此更换胶片或相纸后你的调整仍然保留。",
  "Change how the film sees light": "改变胶片感受光线的方式",
  "This changes how much light reaches the simulated film, affecting colour, contrast, and grain. With the enlarger’s Exposure compensation on, print brightness stays roughly steady. To make the photo lighter or darker, use Prep’s Exposure instead.":
    "它改变到达模拟胶片的光量，影响色彩、对比度和颗粒。放大机的曝光补偿开启时，放印亮度大体保持不变。想让照片更亮或更暗，请改用预处理的曝光。",
  "Small film, bigger texture": "胶片越小，纹理越大",
  "Smaller film needs more enlargement to make the same-sized picture, so grain and glow appear larger. Lower this value for more visible texture; raise it for a finer look. It doesn’t crop your photo.":
    "胶片越小，放大到同样尺寸的倍率越高，颗粒和光晕看起来就越大。调低此值让纹理更明显，调高则更细腻。它不会裁剪你的照片。",
  "Let highlights glow": "让高光泛起光晕",
  "Some light passes through film and reflects back into it, creating a warm glow around bright highlights. Raise Amount for a stronger glow, or Spatial scale to spread it wider. Look around lamps, reflections, and bright edges.":
    "部分光线穿过胶片后又反射回来，在明亮高光周围形成温暖的光晕。调高“数量”增强光晕，或调大“空间范围”让它散得更开。留意灯泡、反光和明亮的边缘。",
  "Soften with a mist filter": "用柔光镜柔化",
  "Photographers put diffusion filters in front of the lens so highlights bloom into a soft halo and shadows lift a little. A Black Pro-Mist is on here: switch Lens Diffusion off and on to compare, try another Filter, or raise Strength for a stronger glow.":
    "摄影师在镜头前加柔光镜，让高光晕开成柔和的光斑，阴影略微提亮。这里开着一枚 Black Pro-Mist（黑柔滤镜）：开关“镜头柔光”对比效果，换一种滤镜，或调高强度获得更明显的光晕。",
  "Colour shaped by chemistry": "化学塑造色彩",
  "During development, chemicals released in one colour layer slow development in nearby layers. This helps separate colours and makes edges look crisper. Lower Amount for gentler colour and less edge contrast.":
    "显影过程中，某一色彩层释放的化学物质会抑制邻近层的显影。这有助于分离色彩，并让边缘看起来更锐利。调低“数量”可获得更柔和的色彩和更弱的边缘反差。",
  "Find every control": "找到所有控件",
  "The Lab shows the controls most photos need. Turn on Advanced controls in Settings to see every camera, film, print and scan setting, like grain size, preflash and the scanner.":
    "暗房显示大多数照片需要的控件。在设置中打开高级控件，即可看到全部相机、胶片、放印和扫描设置，比如颗粒大小、预闪和扫描仪。",
  "Open ⚙ Settings → Advanced controls.": "打开 ⚙ 设置 → 高级控件。",
  "Keep a look you like": "留住你喜欢的风格",
  "Save your settings as a preset to reuse them on other photos. Export it to share the look, or send it as a link.":
    "把你的设置保存为预设，即可在其他照片上复用。可以导出分享，或生成链接发送。",
  "Open ⋯ → Export preset. Presets include Lab settings, not your photo, Prep adjustments, crop or border.":
    "打开 ⋯ → 导出预设。预设包含暗房设置，不含照片、预处理调整、裁剪或边框。",

  // ---- 引导提示与状态 ----------------------------------------------------------------

  "Some settings are hidden. ": "部分设置已隐藏。",
  "Settings › Advanced controls": "设置 › 高级控件",
  " shows them all.": "可显示全部。",
  "Starting…": "启动中…",
  "{backend} · {n} threads": "{backend} · {n} 线程",
  "{backend} · 1 thread": "{backend} · 1 线程",
  "Developing": "显影中",
  "Downloading": "下载中",
  "Downloading {percent}": "下载中 {percent}",
  "Loading Original": "正在加载原图",
  "Preview": "预览",
  "shown at {percent}": "按 {percent} 显示",
  "The preview shows {percent} of these pixels. {note}; inspect grain in an exported file.\n{stages}":
    "预览显示了其中 {percent} 的像素。{note}；请在导出的文件中查看颗粒。\n{stages}",
  "Exports here develop {width} × {height}; a desktop browser exports all of them": "此处导出以 {width} × {height} 显影，桌面浏览器会导出全部像素",
  "Exports develop all of them": "导出会显影全部像素",
  "Matching…": "匹配中…",
  "Starts at {value} EV to match the camera.": "为匹配相机亮度，起始曝光为 {value} EV。",
  "Starts at {value} EV to match the camera, held back to protect highlights.": "为匹配相机亮度，起始曝光为 {value} EV（为保护高光有所收敛）。",
  "Click to type a value": "点击输入数值",
  "Reset to {value}": "重置为 {value}",
  "Reset {title}": "重置{title}",
  "About {label}": "关于{label}",
  "{title} on or off": "{title} 开或关",
  "{title} border": "{title}边框",
  "Reset Adjustments": "重置调整",
  "Restore the defaults while keeping the film, paper and rotation.": "恢复默认设置，但保留胶片、相纸和旋转。",
  "35mm needs a 3:2 or 3:4 photograph. This one is {ratio}.": "35mm 需要 3:2 或 3:4 的照片，而这张是 {ratio}。",
  "Downloading the {style} border… {percent}": "正在下载{style}边框… {percent}",

  // ---- 消息 -----------------------------------------------------------------------

  "Render failed": "渲染失败",
  "Export failed": "导出失败",
  "Balance Print failed": "放印平衡失败",
  "Print balanced: magenta {m}, yellow {y}": "放印已平衡：品红 {m}，黄 {y}",
  "Couldn't open {name}": "无法打开 {name}",
  "Rendering {width}×{height} for export…": "正在以 {width}×{height} 渲染导出…",
  "Exported.": "已导出。",
  "Save or share…": "保存或分享…",
  "Apply or cancel the crop before exporting": "导出前请先应用或取消裁剪",
  "The engine didn't start: {message}": "引擎未能启动：{message}",
  "the engine crashed ({error})": "引擎已崩溃（{error}）",
  "the engine crashed: {message}": "引擎已崩溃：{message}",
  "the engine stopped": "引擎已停止",
  "WebGPU isn't available here ({message}); using the CPU engine.": "此环境无法使用 WebGPU（{message}），改用 CPU 引擎。",
  " Trying again without presenting from the GPU.": " 正在改为不由 GPU 呈现，重试一次。",
  " Switched to the CPU engine.": " 已切换到 CPU 引擎。",
  "{what}: {message}": "{what}：{message}",
  "{what}: {message}.{note}": "{what}：{message}。{note}",
  "The RAW white balance couldn't be applied": "无法应用 RAW 白平衡",
  "No reliable brightness match with these settings.": "当前设置下无法可靠地匹配亮度。",
  "The camera preview couldn’t be read.": "无法读取相机预览。",
  "The original couldn't be shown: {message}": "无法显示原图：{message}",
  "The border couldn't be downloaded: {message}": "边框下载失败：{message}",
  "Negs ran out of memory and reloaded. Film and paper previews are now off to save memory.":
    "Negs 内存不足，已重新加载。为节省内存，胶片与相纸预览已关闭。",
  "Couldn't load sample choices: {message}": "无法加载示例列表：{message}",
  "Sample {n}": "示例 {n}",
  "Open the next sample photo": "打开下一张示例照片",
  "Connect to switch to another sample": "连接网络后才能切换示例",

  // ---- 离线、更新与安装 --------------------------------------------------------------

  "Saving Negs for offline use…": "正在保存 Negs 供离线使用…",
  "Saving Negs for offline use": "正在保存 Negs 供离线使用",
  "Saving everything for offline use": "正在保存全部内容供离线使用",
  "Downloading a new version": "正在下载新版本",
  "{what}… {percent}": "{what}… {percent}",
  "Offline · {size} saved": "离线 · 已保存 {size}",
  "Save all ({size})": "全部保存（{size}）",
  "Every border and spectral table, and the sample photo, for use offline": "全部边框、光谱表和示例照片，供离线使用",
  "Clear": "清除",
  "Remove what Negs saved for offline use": "移除 Negs 保存的离线内容",
  "Removed what Negs saved for offline use. It's saved again the next time Negs opens.":
    "已移除 Negs 保存的离线内容，下次打开时会重新保存。",
  "Not everything could be saved for offline use: {error}": "部分内容无法保存供离线使用：{error}",
  "The new version couldn't be downloaded": "新版本下载失败",
  "Couldn't save Negs for offline use": "无法保存 Negs 供离线使用",
  "{text} ({error}). It tries again the next time Negs opens.": "{text}（{error}）。下次打开 Negs 时会重试。",
  "A new version of Negs is ready.": "Negs 新版本已就绪。",
  "Negs was updated in another window.": "Negs 已在另一个窗口更新。",
  "Reload": "重新加载",
  "{text} Reload to use it.": "{text}重新加载即可使用。",
  "Reloads Negs; the photograph you have open closes": "重新加载 Negs，当前打开的照片会关闭",
  "Use Negs as an app, offline too.": "将 Negs 安装为应用，离线也能用。",
  "Use Negs as an app: in Safari's Share menu, choose Add to Home Screen.":
    "将 Negs 安装为应用：在 Safari 的分享菜单中选择“添加到主屏幕”。",
  "Use Negs as an app: File → Add to Dock.": "将 Negs 安装为应用：文件 → 添加到程序坞。",
  "Install app": "安装应用",
  "Don't show again": "不再显示",

  // ---- 离开提示 --------------------------------------------------------------------

  "Leave without exporting?": "图片还未导出，确定要离开吗？",
  "Leave": "离开",
  "Stay": "留下",
  "Your edits to this photo haven't been exported. Negs doesn't keep them, so leaving discards them.":
    "你对这张照片的编辑尚未导出。Negs 将不会保存它们。",

  // ---- 控件提示（help.js，鼠标悬停 (i)）---------------------------------------------

  "The film the photograph is shot on: its colour response, contrast, grain and halation, from the stock’s measured data.":
    "拍摄这张照片所用的胶片：它的色彩响应、对比度、颗粒和光晕，均来自该型号的实测数据。",
  "The paper the negative is printed on: its contrast, colour and blacks. Changing the film picks the paper it was made for.":
    "负片放印所用的相纸：它的对比度、色彩和黑场。更换胶片时会自动选择与之配套的相纸。",
  "How the RAW’s colours are balanced before the film. As Shot uses the camera’s setting; Daylight and Tungsten balance for that light; Custom sets temperature and tint. Presets set it too, as they do in the Python app.":
    "在胶片之前，RAW 的色彩如何平衡。“拍摄时设置”沿用相机数据；日光和钨丝灯按相应光源平衡；自定义则直接设定色温和色调。预设也会设置它，与 Python 版一致。",
  "The colour of the light to balance for, in kelvin. Lower makes the photograph cooler, higher warmer; 6504 K is Daylight.":
    "所要平衡的光源颜色，单位开尔文。调低照片偏冷，调高偏暖；6504 K 即日光。",
  "1 is neutral. Higher is greener, lower more magenta.": "1 为中性。调高偏绿，调低偏品红。",
  "Which part of the photograph the meter reads. Average and Median read the whole frame; Center-Weighted, Partial and Multi-Zone favour the centre; Matrix weighs a 5 × 5 grid; Highlight-Weighted favours bright areas, to keep them from clipping.":
    "测光读取照片的哪一部分。平均和中值读取整个画面；中央重点、局部和多区偏向中央；矩阵按 5 × 5 网格加权；高光重点偏向亮部，以防溢出。",
  "Darkens (left) or brightens (right) the brightest parts of the scene before the film sees them. Far left recovers highlights that would otherwise crowd the film’s shoulder.":
    "在胶片“看到”场景之前，压暗（左）或提亮（右）最亮的部分。拉到最左可挽回那些本会挤入胶片肩部的高光。",
  "Brightens (right) or darkens (left) the darkest parts of the scene before the film sees them.":
    "在胶片“看到”场景之前，提亮（右）或压暗（左）最暗的部分。",
  "Spreads the scene’s tones apart or draws them together around the mid-tones, before the film adds its own contrast.":
    "在胶片加上自身反差之前，把场景的影调围绕中间调拉开或收拢。",
  "Raises or lowers the scene’s blacks, with less effect on brighter tones.": "抬高或压低场景的黑场，对较亮影调影响较小。",
  "Brightens or darkens the scene’s brighter tones, with less effect on the blacks.": "提亮或压暗场景中较亮的影调，对黑场影响较小。",
  "Strengthens or mutes muted colours more than colours that are already strong, before the film.":
    "在胶片之前，对本来就不饱和的颜色增强或弱化得更多，对已经浓烈的颜色影响较小。",
  "Strengthens or mutes all colours evenly, before the film.": "在胶片之前，均匀地增强或减弱所有色彩。",
  "Local contrast in the mid-tones: right adds texture and clarity, left softens it.":
    "中间调的局部对比度：向右增加纹理和清晰度，向左使其柔化。",
  "The file holds linear values, without the colour space’s curve.": "文件保存的是线性值，不含色彩空间的曲线。",
  "The colour space the file’s values are in.": "文件数值所处的色彩空间。",
  "Decodes the colour space’s curve (its gamma) to linear light. Turn it off for files that are already linear.":
    "把色彩空间的曲线（伽马）解码为线性光。文件本身已是线性时请关闭。",
  "Over- or underexposes the film. With Enlarger › Exposure compensation on (the default), the print makes up for it: mid-tones keep their brightness while the negative’s density, grain and highlight and shadow detail change. To make the photograph brighter, use Prep › Exposure.":
    "让胶片过曝或欠曝。放大机的曝光补偿开启（默认）时，放印会随之补偿：中间调保持亮度，而负片的密度、颗粒和高光阴影细节随之变化。想让照片更亮，请用预处理 › 曝光。",
  "The film the photograph was taken on, by the frame’s long edge. Grain, halation, scatter and diffusion are all sized on the film, so a smaller format shows them larger.":
    "拍摄所用的胶片规格，按画幅长边计。颗粒、光晕、散射和柔光都按胶片上的实际尺寸计算，因此规格越小它们看起来越大。",
  "Softness of the taking lens, measured on the film. 0 is a perfect lens.": "拍摄镜头的柔和程度，以胶片上的尺度衡量。0 为理想镜头。",
  "The filter in front of the lens. Glimmerglass is the subtlest and tightest; Black Pro-Mist a concentrated, warm halo that also lifts the shadows; Pro-Mist a wider glow; CineBloom the widest, warmest bloom.":
    "镜头前的柔光镜。Glimmerglass 最轻最收敛；Black Pro-Mist 是集中而偏暖的光晕，同时略微提亮阴影；Pro-Mist 光晕更宽；CineBloom 最宽、暖调最重。",
  "The filter’s grade, as sold: 1/8, 1/4, 1/2, 1 and 2. More spreads more light into the halo, which also lifts shadows and lowers contrast.":
    "滤镜的市售规格：1/8、1/4、1/2、1 和 2。数值越大散入光晕的光越多，同时提亮阴影、降低对比。",
  "How far the halo spreads from each highlight, without changing how much light it takes.":
    "光晕从每个高光向外散布的距离，不改变它分走的光量。",
  "Warms (right) or cools (left) the halo, on top of the filter’s own tint. 0 keeps the filter’s.":
    "在滤镜自身色调的基础上，让光晕偏暖（右）或偏冷（左）。0 保持滤镜原样。",
  "The share of diffused light kept close to each highlight. 1 is the filter’s own; the core, halo and bloom shares are balanced to add up.":
    "扩散光中留在每个高光近处的份额。1 为滤镜原值；核心、光晕和泛光的份额加权后平衡为 1。",
  "The share of diffused light in the mid-sized halo. 1 is the filter’s own.": "扩散光在中等光晕中的份额。1 为滤镜原值。",
  "The share of diffused light in the wide bloom. 1 is the filter’s own.": "扩散光在宽泛光晕中的份额。1 为滤镜原值。",
  "The mid-sized halo’s radius. 1 is the filter’s own.": "中等光晕的半径。1 为滤镜原值。",
  "The wide bloom’s radius. 1 is the filter’s own.": "宽泛光晕的半径。1 为滤镜原值。",
  "The film’s development contrast. 1 is normal development; higher is like pushing, lower like pulling.":
    "胶片的显影反差。1 为正常显影；调高相当于增感冲洗，调低相当于减感冲洗。",
  "Contrast of the film’s fastest, most light-sensitive layer, which records the shadows.":
    "胶片感光最快、最灵敏的那一层的反差，它记录阴影。",
  "Contrast of the film’s medium and slow layers, which record the mid-tones and highlights.":
    "胶片中速层和慢速层的反差，它们记录中间调和高光。",
  "Contrast of the film’s red-sensitive (cyan) layer alone. Uneven contrast between layers gives shadows and highlights opposite colour casts.":
    "仅胶片感红（青成色）层的反差。各层反差不均会让阴影和高光出现相反的偏色。",
  "Contrast of the film’s green-sensitive (magenta) layer alone.": "仅胶片感绿（品红成色）层的反差。",
  "Contrast of the film’s blue-sensitive (yellow) layer alone.": "仅胶片感蓝（黄成色）层的反差。",
  "Developer running out in dense, brightly exposed areas: the highlights roll off more gently, mid-grey stays put. 0 is off.":
    "显影液在曝光浓密的亮部逐渐耗尽：高光更柔和地滚降，中灰保持不变。0 为关闭。",
  "How coarse the grain is, from the film’s datasheet (RMS granularity). Fine films measure about 4, coarse ones 40.":
    "颗粒的粗细程度，来自胶片数据页（RMS 颗粒度）。细颗粒胶片约为 4，粗颗粒约为 40。",
  "How evenly the crystals are spread. Higher makes the grain peak in the mid-tones and ease off in the densest areas; lower lets it keep growing into them.":
    "银盐晶体分布的均匀程度。调高后颗粒在中间调最强，在最密处减弱；调低则颗粒在最密处继续增强。",
  "The crystal size of one of the emulsion’s three sub-layers, relative to the film’s own. Larger crystals make coarser, stronger grain.":
    "乳剂三个子层之一的晶体尺寸，相对于胶片自身。晶体越大，颗粒越粗、越明显。",
  "How far the dye cloud around each developed crystal spreads, on the film. Visible only at high magnification.":
    "每个已显影晶体周围染料云在胶片上的扩散距离。只有高倍放大时才可见。",
  "Softens the grain after it’s drawn, in pixels. Raise it for large exports, where a crystal covers several pixels; lower it for small ones.":
    "颗粒生成后再作柔化，以像素计。大幅面导出时一个晶体覆盖多个像素，宜调高；小图宜调低。",
  "Restores the sharpness the grain blur takes away, without adding noise. Too much looks crunchy; 0 is off.":
    "找回颗粒柔化损失的锐度，且不引入噪点。过强会显得干涩；0 为关闭。",
  "The radius of that sharpening, in pixels.": "该锐化的半径，以像素计。",
  "Blur of the grain’s finest structure, in µm. Only for extreme magnification.": "颗粒最细微结构的模糊，以 µm 计。仅用于极高倍放大。",
  "Size of the molecular clumps within the grain, in nm. Only for extreme magnification.": "颗粒内部分子团簇的大小，以 nm 计。仅用于极高倍放大。",
  "The red glow around bright highlights, from light reflecting off the back of the film base. 1 is the film’s own; films with little or no anti-halation layer glow most.":
    "明亮高光周围的红色光晕，来自片基背面的反射光。1 为胶片原值；防光晕层薄弱或没有的胶片光晕最强。",
  "How far the halation glow spreads. 1 is the film’s own.": "光晕散布的距离。1 为胶片原值。",
  "Light scattering sideways inside the emulsion, which softens the whole frame slightly. 1 is the film’s own and the most; lower is sharper than the film.":
    "光在乳剂内横向散射，让整个画面略微柔化。1 为胶片原值，也是最大值，调低会比胶片更锐。",
  "How far that scatter spreads. 1 is the film’s own; above it the whole frame softens quickly.":
    "散射的散布的距离。1 为胶片原值；再调高整个画面会迅速柔化。",
  "How many times light bounces between the emulsion and the back of the base. Typically 2 or 3.":
    "光在乳剂与片基背面之间的反射次数。通常为 2 或 3。",
  "How much of the glow each bounce keeps. Lower fades the later bounces faster.":
    "每次反射保留的光晕比例。调低会让后几次反射衰减更快。",
  "Development inhibitor couplers: each colour layer holds back the others as it develops, which gives colour negative film its saturation and edge sharpness. 1 is the film’s own, 0 off.":
    "显影抑制成色剂：各色彩层显影时会抑制其他层，这赋予彩色负片饱和度和边缘锐度。1 为胶片原值，0 为关闭。",
  "How far the inhibitor spreads in the emulsion, in µm. Wider makes bright areas hold back development past their edges, which reads as extra clarity; 5–20 µm is the useful range.":
    "抑制剂在乳剂中扩散的距离，以 µm 计。调大后亮部会抑制其边界以外的显影，看起来更通透；有效范围 5–20 µm。",
  "The length of a faint, long-range tail on that spread, in µm, which gives large bright areas a broad, gentle falloff.":
    "上述扩散附带的微弱长程拖尾的长度，以 µm 计，让大片亮部呈现宽而平缓的衰减。",
  "The share of the inhibitor in the long tail. 0 removes the tail.": "长拖尾中抑制剂的份额。0 即去掉拖尾。",
  "The density of the film base and fog (the orange mask of colour negatives). 1 is the film’s own. After changing it, use Balance Print.":
    "片基加灰雾的密度（彩色负片的橙色蒙罩）。1 为胶片原值。改动后请使用放印平衡。",
  "Tilts the base’s density across the spectrum: right adds density in the reds and takes it from the blues, left the reverse.":
    "让片基密度沿光谱倾斜：向右在红端增密度、蓝端减密度，向左相反。",
  "Adds or removes cyan in the film base. 1 is neutral.": "在片基中增减青色。1 为中性。",
  "Adds or removes magenta in the film base. 1 is neutral.": "在片基中增减品红。1 为中性。",
  "Adds or removes yellow in the film base. 1 is neutral.": "在片基中增减黄。1 为中性。",
  "The enlarger’s exposure time. 1 is a normal print; higher prints darker, lower lighter, as with a real enlarger.":
    "放大机的曝光时间。1 为正常放印；调高印得更深，调低更浅，与真实放大机一致。",
  "Magenta filtration of the enlarger, in Kodak CC units from neutral. More magenta in the light makes the print greener; less makes it more magenta.":
    "放大机的品红滤色，以柯达 CC 值偏离中性计。光源中品红越多照片越偏绿，越少越偏品红。",
  "Yellow filtration of the enlarger, in Kodak CC units from neutral. More yellow in the light makes the print bluer; less makes it more yellow.":
    "放大机的黄滤色，以柯达 CC 值偏离中性计。光源中黄越多照片越偏蓝，越少越偏黄。",
  "Prints the negative to the same mid-grey whatever its Negative exposure, as a printer would. Off, Negative exposure changes the print’s brightness, as with a fixed enlarger setting.":
    "无论负片曝光如何，都把负片放印到相同的中灰，如同自动放印机会做的。关闭后，负片曝光会改变放印亮度，如同放大机参数固定。",
  "A brief, even exposure of the paper before the print: lifts the deepest shadows and lowers contrast. 0 is off.":
    "放印前对相纸的一次短暂均匀曝光：提亮最深的阴影并降低对比。0 为关闭。",
  "Magenta filtration for the preflash alone, in Kodak CC units, to tint the lifted shadows.":
    "仅预闪光的品红滤色，以柯达 CC 值计，用于给提亮的阴影染色。",
  "Yellow filtration for the preflash alone, in Kodak CC units, to tint the lifted shadows.":
    "仅预闪光的黄滤色，以柯达 CC 值计，用于给提亮的阴影染色。",
  "The filter under the enlarger lens. Glimmerglass is the subtlest and tightest; Black Pro-Mist a concentrated, warm halo; Pro-Mist a wider glow; CineBloom the widest, warmest bloom. On a print the glow spreads from the dark areas, the negative’s brightest.":
    "放大机镜头下的柔光镜。Glimmerglass 最轻最收敛；Black Pro-Mist 是集中偏暖的光晕；Pro-Mist 光晕更宽；CineBloom 最宽、暖调最重。放印照片上，光晕从暗部（即负片最亮处）散开。",
  "The filter’s grade, as sold: 1/8, 1/4, 1/2, 1 and 2. More spreads more light into the halo, which softens the print and lowers its contrast.":
    "滤镜的市售规格：1/8、1/4、1/2、1 和 2。数值越大散入光晕的光越多，放印更柔、对比更低。",
  "How far the halo spreads, without changing how much light it takes.": "光晕散布的距离，不改变它分走的光量。",
  "The share of diffused light kept close to its source. 1 is the filter’s own; the core, halo and bloom shares are balanced to add up.":
    "扩散光中留在光源近处的份额。1 为滤镜原值；核心、光晕和泛光的份额加权后平衡为 1。",
  "The core’s radius. 1 is the filter’s own.": "核心的半径。1 为滤镜原值。",
  "The paper’s contrast, like a paper grade. 1 is the paper’s own.": "相纸的反差，类似纸号。1 为相纸原值。",
  "Contrast of the paper’s fastest layer, which prints the photograph’s highlights.": "相纸感光最快的那一层的反差，它印出照片的高光。",
  "Contrast of the paper’s medium and slow layers, which print the mid-tones and shadows.":
    "相纸中速层和慢速层的反差，它们印出中间调和阴影。",
  "Contrast of the paper’s red-sensitive (cyan) layer alone. Uneven layers give shadows and highlights opposite casts that filtration can’t fix.":
    "仅相纸感红（青成色）层的反差。各层不均会让阴影和高光出现滤色无法修正的相反偏色。",
  "Contrast of the paper’s green-sensitive (magenta) layer alone.": "仅相纸感绿（品红成色）层的反差。",
  "Contrast of the paper’s blue-sensitive (yellow) layer alone.": "仅相纸感蓝（黄成色）层的反差。",
  "Developer running out in the print’s dense, dark areas: the shadows roll off more gently, mid-grey stays put. 0 is off.":
    "显影液在照片浓密的暗部逐渐耗尽：阴影更柔和地滚降，中灰保持不变。0 为关闭。",
  "The density of the paper’s base, its whitest white. 1 is the paper’s own.": "相纸纸基的密度，即它的最白。1 为相纸原值。",
  "Scales the cyan in the paper’s base. 1 is the paper’s own.": "缩放纸基中的青。1 为相纸原值。",
  "Scales the magenta in the paper’s base. 1 is the paper’s own.": "缩放纸基中的品红。1 为相纸原值。",
  "Scales the yellow in the paper’s base. 1 is the paper’s own.": "缩放纸基中的黄。1 为相纸原值。",
  "A faint veil of room light reflecting off the print’s surface, which lifts the deepest blacks slightly.":
    "室内光在照片表面反射形成的一层薄纱，让最深的黑色略微浮起。",
  "How unevenly the veil falls across the print, as on a textured surface.": "这层薄纱落在照片上的不均匀程度，如同落在粗糙的表面上。",
  "Softens the veil’s unevenness, in pixels.": "柔化薄纱的不均匀分布，以像素计。",
  "Softness of the scanner’s lens, in pixels. 0 is off.": "扫描仪镜头的柔和程度，以像素计。0 为关闭。",
  "Takes the colour out of the grain, as a lab scanner’s software does: the grain keeps its texture, but its red, green and blue specks turn neutral. Colour edges stay put. 0 is off.": "像冲印店扫描仪的软件那样去除颗粒中的色彩：颗粒保留其质感，但红、绿、蓝色斑变为中性。色彩边缘保持不变。0 为关闭。",
  "The radius of the scanner’s sharpening, in pixels.": "扫描仪锐化的半径，以像素计。",
  "The strength of the scanner’s sharpening. 0 is off, for sharpening later in another app.":
    "扫描仪锐化的强度。0 为关闭，留待之后在其他应用中锐化。",
  "Sets the scan’s lightest tone to White level. Off, the highlights stay where the paper puts them. Mostly for slides.":
    "把扫描最亮的色调设为白场电平。关闭时高光停留在相纸给出的位置。主要用于反转片。",
  "The lightest tone with White correction on. 1 is full white.": "白场校正开启时的最亮色调。1 为纯白。",
  "Sets the scan’s darkest tone to Black level. Off, the shadows stay where the paper puts them. Mostly for slides.":
    "把扫描最暗的色调设为黑场电平。关闭时阴影停留在相纸给出的位置。主要用于反转片。",
  "The darkest tone with Black correction on. 0 is true black; higher lifts and fades the blacks.":
    "黑场校正开启时的最暗色调。0 为纯黑；调高会抬起并稀释黑场。",
  "Rolls colours the output colour space can’t hold back inside it, leaving colours within it untouched. ACES RGC is the industry standard; OKLCh, OKLrab and CAM16-UCS keep hue more evenly. Off clips them.":
    "把输出色彩空间装不下的颜色收拢回其内部，空间内的颜色不受影响。ACES RGC 是行业标准；OKLCh、OKLrab 和 CAM16-UCS 对色相的保护更均匀。关闭则直接裁切。",
  "Pulls the photograph’s colours that lie outside what eyes can see (possible in wide-gamut RAWs) inside, before they become spectra. Off passes them as they are.":
    "把照片中超出人眼可见范围的颜色（广色域 RAW 中可能出现）拉回可见范围，再转换成光谱。关闭则原样通过。",
  "Adapts the photograph’s white to the light the film was characterised in, with CAT16. Off uses CAT02.":
    "用 CAT16 把照片的白点适配到胶片标定时所处的光源。关闭则用 CAT02。",
  "How each colour is turned into the spectrum of light the film is exposed to. Hanatos 2025 covers every visible colour; Mallett 2019 only sRGB, clipping the rest; Arctic 2026 is a newer method in testing.":
    "每种颜色如何转换成照射胶片的光谱。Hanatos 2025 覆盖所有可见色；Mallett 2019 仅覆盖 sRGB，其余被裁切；Arctic 2026 是测试中的新方法。",
  "The filter’s grade, as sold: 1/8, 1/4, 1/2, 1 and 2. More spreads more light into the halo, which also lifts shadows and lowers contrast.":
    "滤镜的市售规格：1/8、1/4、1/2、1 和 2。数值越大散入光晕的光越多，同时提亮阴影、降低对比。",
  "Meters the photograph and exposes the film so the metered area lands on mid-grey, as a camera's meter does. Exposure adds to it.":
    "对照片测光并曝光胶片，使被测区域落在中灰上，如同相机的测光那样。曝光在其基础上叠加。",
  "How long the film develops, from the times the stock was measured at. Longer gives more density and contrast.":
    "胶片的显影时间，选自该型号的标定时间。更长则密度和反差更高。",
  "How long the paper develops, from the times it was measured at. Longer gives deeper blacks and more contrast.":
    "相纸的显影时间，选自其标定时间。更长则黑场更深、反差更高。",

  // ---- 分组提示（help.js GROUPS）---------------------------------------------------

  "A diffusion filter on the camera lens: highlights bloom into a soft halo.": "镜头前的柔光镜：高光晕开成柔和的光斑。",
  "Shapes how the film develops: its contrast, per layer and per colour.": "塑造胶片的显影：各层、各色的反差。",
  "The film’s silver grain, drawn from the stock’s measured granularity.": "胶片的银盐颗粒，按型号实测颗粒度生成。",
  "Light scattering inside the film and bouncing back off its base: a red glow around bright highlights.":
    "光在胶片内散射并从片基背面弹回：明亮高光周围的红色光晕。",
  "Chemical crosstalk between the film’s colour layers: saturation and edge sharpness.":
    "胶片各色彩层之间的化学串扰：带来饱和度和边缘锐度。",
  "The colour and density of the clear film, the orange mask of colour negatives.":
    "透明片基的色彩与密度，即彩色负片的橙色蒙罩。",
  "Prints the negative: exposure time, colour filtration and preflash.": "把负片放印成照片：曝光时间、滤色和预闪。",
  "Shapes how the paper develops: its contrast, per layer and per colour.": "塑造相纸的显影：各层、各色的反差。",
  "The colour and density of the paper itself, its whitest white.": "相纸本身的色彩与密度，即它的最白。",
  "Room light reflecting off the print’s surface.": "室内光在照片表面的反射。",
  "Scans the finished print: sharpness, colour noise, and black and white points.": "扫描完成的照片：锐度、彩色噪点和黑白场。",
  "How colours enter and leave the spectral simulation.": "颜色如何进出光谱模拟。",
  "How the file’s values are read: its colour space and curve.": "文件数值如何读取：色彩空间与曲线。",
  "The colour space the preview develops in.": "预览显影所用的色彩空间。",
  "A diffusion filter on the enlarger lens: the print glows softly around its shadows.":
    "放大机镜头下的柔光镜：照片在暗部周围泛起柔光。",

  // ---- 边框样式提示（border.js）----------------------------------------------------

  "A plain mat around the photograph.": "照片周围的一圈素色卡纸。",
  "A scanned 120 film frame with an irregular black edge.": "扫描的 120 胶片框，边缘呈不规则的黑色。",
  "A mat with a thin line around the photograph.": "卡纸装裱，照片四周有一圈细线。",
  "A 35mm negative strip with the film’s edge print. Needs a 3:2 or 3:4 photograph.":
    "带边缘印字的 35mm 负片条。需要 3:2 或 3:4 的照片。",
  "Solid is a plain mat; Keyline adds a thin line around the photograph; Film is a scanned 120 film frame; 35mm a negative strip with the film’s edge print.":
    "纯色是素色卡纸；细线框在照片四周加一圈细线；胶片框是扫描的 120 胶片框；35mm 是带边缘印字的底片条。",
  "Frames the photograph in a mat or a film frame. The preview always shows it; Include in export decides whether files get it.":
    "用卡纸或胶片框装裱照片。预览始终显示边框；“包含在导出中”决定文件是否带上它。",
  "The mat’s width, in percent of the photograph’s long edge.": "卡纸的宽度，按照片长边的百分比。",
  "The line’s width, in percent of the photograph’s long edge.": "细线的宽度，按照片长边的百分比。",
  "The mat’s colour.": "卡纸的颜色。",
  "The line’s colour.": "细线的颜色。",
  "Adds black around the film frame, in percent of the photograph’s long edge.": "在胶片框周围加黑边，按照片长边的百分比。",
  "Adds white above and below, or at the sides, so Instagram shows the whole photograph in a 3:4 portrait post.":
    "在上下或两侧加白，让 Instagram 的 3:4 竖版帖子能显示完整照片。",
  "Adds the border to exported photographs and saved previews.": "把边框加进导出的照片和保存的预览。",

  // ---- 裁剪比例（crop.js）----------------------------------------------------------

  "Square · 6×6": "正方形 · 6×6",

  // ---- Prep › Exposure 的两种说明（app.js helpFor）----------------------------------

  "Brighter or darker than the match, in stops. It stays when the match changes with the film, paper or a preset.":
    "相对匹配亮度的增减，以挡为单位。随胶片、相纸或预设改变匹配时，它会保留。",
  "Brightens or darkens the scene before the film, in stops. Unlike Negative exposure, the print doesn’t make up for it.":
    "在胶片之前提亮或压暗场景，以挡为单位。与负片曝光不同，放印不会将其补偿回来。",
};
