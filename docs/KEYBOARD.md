# Keymap reference / 快捷键参考

This reference is generated from RustView’s own dispatcher and project-authored command labels. It records default bindings, not a claim of complete GUI acceptance. 自定义设置可能覆盖下列默认值；文本输入与独立对话框优先处理自身按键。

`Num` identifies the numeric keypad. `Character` identifies a text-character accelerator, which is intentionally distinct from a physical key. `Num` 表示数字小键盘；字符事件与物理按键分别处理。

## Runtime additions / 运行时补充

| Context / 模式 | Key / 按键 | Behavior / 行为 |
|---|---|---|
| Manage / 管理 | Enter | Open the focused item / 打开焦点项目 |
| View / 查看 | G | Pause, resume or replay GIF/WebP / 图像动画暂停、恢复、重播 |
| View / 查看 | E | Exposure clipping overlay / 曝光提示覆盖层 |
| View / 查看 | Ctrl+Left / Ctrl+Right | Previous / next image / 上一张、下一张 |

Alt+S / Pause retains no slideshow behavior: slideshow support is intentionally absent. 不启用自动翻图幻灯片。

Compare uses its own bindings and then falls back to View. The processing workspace uses its local apply binding and then View bindings; text fields retain text-edit shortcuts. 比较模式先处理本地按键，再查查看模式；处理工作区的 Ctrl+A 在无文本焦点时应用工具参数。

## Manage / 管理

| Key / 按键 | RustView command / 功能 | ID |
|---|---|---|
| Alt+I | 打开目录筛选区域 | 12262 |
| F4 | 输入目录位置 | 2194 |
| Ctrl+B | 加入图片收藏集 | 2144 |
| Character '[' | 切换ICC色彩管理 | 11151 |
| Ctrl+C | 复制所选文件 | 2104 |
| Ctrl+Insert | 复制所选文件 | 2104 |
| Ctrl+Shift+C | 复制当前像素 | 2036 |
| Alt+C | 所选文件复制到 | 2033 |
| Ctrl+X | 剪切所选文件 | 2103 |
| Delete | 删除所选文件 | 2031 |
| Ctrl+Delete | 删除所选文件 | 2031 |
| Shift+Delete | 删除所选文件 | 2031 |
| Ctrl+Shift+Delete | 删除所选文件 | 2031 |
| Ctrl+Alt+D | 编辑目录内标题 | 2412 |
| Alt+A | 包内文件提取到 | 2061 |
| Ctrl+Shift+I | 反选可见项目 | 2494 |
| Alt+M | 所选文件移动到 | 2034 |
| Ctrl+V | 粘贴剪贴板文件 | 2105 |
| Shift+Insert | 粘贴剪贴板文件 | 2105 |
| F2 | 所选项目改名 | 2032 |
| F3 | 文件名查找区域 | 2035 |
| Ctrl+A | 选择全部可见项目 | 2037 |
| Ctrl+Shift+A | 选择可见文件 | 2038 |
| Ctrl+I | 选择可见图片 | 2039 |
| Ctrl+Shift+D | 取消项目选择 | 2040 |
| Ctrl+Backslash | 选择带挑选标记的图片 | 10657 |
| Ctrl+K | 编辑源图IPTC关键词 | 2436 |
| Alt+K | 编辑目录关键词 | 22298 |
| Ctrl+Alt+X | 使用关联编辑程序 | 2138 |
| Alt+G | 打开设备文件夹 | 2028 |
| Ctrl+W | 退出应用 | 2030 |
| Ctrl+N | 新建浏览窗口 | 2024 |
| Ctrl+O | 选择图片或压缩文件 | 2102 |
| Ctrl+P | 打印所选图片 | 2106 |
| Ctrl+Shift+E | 使用关联编辑程序 | 2186 |
| Ctrl+Alt+E | 格式输出工作区 | 22490 |
| Ctrl+Shift+O | 使用关联查看程序 | 2185 |
| Alt+Right | 目录历史前进 | 2021 |
| Alt+Num - | 分组顺序倒排 | 12172 |
| Alt+Num + | 分组顺序正排 | 12171 |
| F1 | 显示本地操作帮助 | 2211 |
| Alt+X | 从图片收藏集移除 | 2384 |
| Alt+Left | 目录历史后退 | 2172 |
| F12 | 文件资料列布局 | 2012 |
| F7 | 底部胶片栏布局 | 2014 |
| F10 | 紧凑图标布局 | 2010 |
| F11 | 单列名称布局 | 2011 |
| F8 | 网格缩略图布局 | 2009 |
| F6 | 缩略图资料布局 | 2015 |
| F9 | 横向卡片布局 | 2013 |
| Shift+F6 | 焦点移到前一区域 | 57681 |
| Alt+Character ',' | 属性切换前页签 | 2534 |
| Alt+Character '.' | 属性切换后页签 | 2533 |
| Ctrl+5 | 设置星级5 | 2119 |
| Ctrl+Num 5 | 设置星级5 | 2119 |
| Ctrl+4 | 设置星级4 | 2118 |
| Ctrl+Num 4 | 设置星级4 | 2118 |
| Ctrl+1 | 设置星级1 | 2115 |
| Ctrl+Num 1 | 设置星级1 | 2115 |
| Ctrl+3 | 设置星级3 | 2117 |
| Ctrl+Num 3 | 设置星级3 | 2117 |
| Ctrl+2 | 设置星级2 | 2116 |
| Ctrl+Num 2 | 设置星级2 | 2116 |
| Ctrl+0 | 清除星级 | 2114 |
| Ctrl+Num 0 | 清除星级 | 2114 |
| Alt+5 | 设置颜色标签5 | 22328 |
| Alt+Num 5 | 设置颜色标签5 | 22328 |
| Alt+4 | 设置颜色标签4 | 22327 |
| Alt+Num 4 | 设置颜色标签4 | 22327 |
| Alt+1 | 设置颜色标签1 | 22324 |
| Alt+Num 1 | 设置颜色标签1 | 22324 |
| Alt+3 | 设置颜色标签3 | 22326 |
| Alt+Num 3 | 设置颜色标签3 | 22326 |
| Alt+2 | 设置颜色标签2 | 22325 |
| Alt+Num 2 | 设置颜色标签2 | 22325 |
| Alt+0 | 清除颜色标签 | 22323 |
| Alt+Num 0 | 清除颜色标签 | 22323 |
| Ctrl+Shift+L | 打开目录筛选区域 | 2437 |
| Shift+Tab | 焦点移到前一区域 | 2229 |
| Ctrl+Shift+Tab | 焦点移到前一区域 | 2229 |
| Tab | 焦点移到后一区域 | 2235 |
| Ctrl+Tab | 焦点移到后一区域 | 2235 |
| Backslash | 切换挑选标记 | 12021 |
| Alt+L | 打开全局目录记录 | 12238 |
| Ctrl+L | 所选图片曝光处理 | 2046 |
| Ctrl+Alt+B | 所选图片批处理 | 10576 |
| Ctrl+T | 维护文件日期 | 2047 |
| Ctrl+Alt+R | 打开图片对照视图 | 2528 |
| Ctrl+F | 所选图片格式处理 | 2043 |
| Ctrl+D | RAW开发工作区 | 10579 |
| Ctrl+E | 图像处理工作区 | 21902 |
| Ctrl+G | 导出文件清单 | 2048 |
| Alt+O | 选项与按键设置 | 2050 |
| Ctrl+R | 所选图片尺寸处理 | 2045 |
| Ctrl+J | 所选图片方向处理 | 2044 |
| Ctrl+M | 批量维护目录信息 | 2435 |
| Ctrl+Shift+Alt+W | 居中图片壁纸 | 2178 |
| Alt+W | 恢复先前壁纸 | 2180 |
| Ctrl+Alt+W | 拉伸图片壁纸 | 21897 |
| Ctrl+Shift+W | 重复图片壁纸 | 2179 |
| Backspace | 进入父目录 | 2022 |
| Ctrl+Shift+2 | 目录整理区域 | 2002 |
| Shift+F5 | 重新扫描目录 | 2398 |
| Ctrl+Alt+L | 切换目录操作栏 | 2153 |
| Ctrl+Shift+1 | 本地目录树 | 2001 |
| Ctrl+Shift+F | 管理窗口全屏 | 2075 |
| Ctrl+Shift+5 | 打开图片收藏集 | 2008 |
| Shift+Alt+F | 切换目录浏览模式 | 2181 |
| Ctrl+Shift+P | 图片预览区域 | 2006 |
| Alt+Enter | 查看项目属性 | 2003 |
| F5 | 重新扫描目录 | 2143 |
| Ctrl+Shift+3 | 文件名查找区域 | 2007 |
| Num - | 项目顺序倒排 | 2402 |
| Num + | 项目顺序正排 | 2401 |
| Ctrl+Shift+B | 切换底部状态区 | 2151 |
| Ctrl+Shift+7 | 文件任务区域 | 1999 |
| Ctrl+Shift+T | 切换主要操作栏 | 2154 |

## View / 查看

| Key / 按键 | RustView command / 功能 | ID |
|---|---|---|
| Character '*' | 整图适配视区 | 2253 |
| Character '+' | 提高显示比例 | 2238 |
| Alt+Character ',' | 属性切换前页签 | 2534 |
| Character '-' | 降低显示比例 | 2239 |
| Alt+Character '.' | 属性切换后页签 | 2533 |
| Character '/' | 按物理像素显示 | 2254 |
| Alt+0 | 清除颜色标签 | 22323 |
| Alt+1 | 设置颜色标签1 | 22324 |
| Alt+2 | 设置颜色标签2 | 22325 |
| Alt+3 | 设置颜色标签3 | 22326 |
| Alt+4 | 设置颜色标签4 | 22327 |
| Alt+5 | 设置颜色标签5 | 22328 |
| Ctrl+0 | 清除星级 | 2114 |
| Ctrl+1 | 设置星级1 | 2115 |
| Ctrl+Shift+1 | 转为单色二值 | 2315 |
| Ctrl+2 | 设置星级2 | 2116 |
| Ctrl+Shift+2 | 转为四位灰度 | 2316 |
| Ctrl+3 | 设置星级3 | 2117 |
| Ctrl+Shift+3 | 转为八位灰度 | 2317 |
| Ctrl+4 | 设置星级4 | 2118 |
| Ctrl+Shift+4 | 转为四位调色板 | 2318 |
| Ctrl+5 | 设置星级5 | 2119 |
| Ctrl+Shift+5 | 转为八位调色板 | 2319 |
| Ctrl+Shift+6 | 转为RGB555 | 2320 |
| Ctrl+Shift+7 | 转为RGB24 | 2321 |
| Ctrl+Shift+8 | 转为灰度16 | 2322 |
| Ctrl+Shift+9 | 转为RGB48 | 2323 |
| Character '=' | 提高显示比例 | 2238 |
| Character '[' | 切换ICC色彩管理 | 2862 |
| Alt+Character '\\' | 保持浏览缩放比例 | 2277 |
| Character '_' | 降低显示比例 | 2239 |
| Ctrl+A | 切换查看文字模板 | 2375 |
| Ctrl+Shift+A | 显示局部放大窗 | 2313 |
| B | 切换底部状态区 | 2247 |
| Ctrl+B | 加入图片收藏集 | 2381 |
| Ctrl+Alt+B | 所选图片批处理 | 10576 |
| Ctrl+Shift+B | 切换底部状态区 | 2247 |
| Ctrl+C | 复制当前像素 | 2260 |
| Alt+C | 所选文件复制到 | 2274 |
| Ctrl+Shift+C | 复制当前像素 | 2260 |
| Ctrl+Alt+D | 编辑目录内标题 | 2263 |
| Ctrl+D | RAW开发工作区 | 10556 |
| Ctrl+E | 图像处理工作区 | 2356 |
| Shift+E | 使用关联编辑程序 | 2334 |
| Ctrl+Alt+E | 格式输出工作区 | 22490 |
| F | 切换图片全屏 | 2242 |
| Ctrl+F | 格式输出工作区 | 2268 |
| Ctrl+Shift+F | 切换图片全屏 | 2242 |
| Ctrl+Shift+G | 清除自定义平铺背景 | 2308 |
| Ctrl+Shift+H | 显示RGB直方图 | 2088 |
| Ctrl+J | 方向变换工作区 | 2285 |
| Ctrl+K | 编辑源图IPTC关键词 | 2436 |
| Alt+K | 编辑目录关键词 | 22298 |
| L | 保持浏览平移位置 | 2160 |
| Ctrl+L | 曝光处理工作区 | 21851 |
| Shift+L | 使用关联查看程序 | 2233 |
| Shift+Alt+L | 浏览上一张 | 1993 |
| Ctrl+M | 使用默认目录元数据预设 | 21853 |
| Alt+M | 所选文件移动到 | 2275 |
| Ctrl+O | 选择图片或压缩文件 | 2258 |
| Alt+O | 选项与按键设置 | 2230 |
| Ctrl+P | 打印所选图片 | 2259 |
| Ctrl+Shift+P | 打印全部所选图片 | 2282 |
| Ctrl+Q | 取消矩形选区 | 2279 |
| Ctrl+Shift+Q | 编辑查看文字模板 | 2303 |
| Ctrl+R | 尺寸处理工作区 | 21852 |
| Alt+R | 所选项目改名 | 2262 |
| Ctrl+S | 保存当前处理结果 | 10420 |
| Ctrl+Shift+S | 显示全图定位窗 | 2314 |
| Ctrl+Alt+S | 另存处理副本 | 22294 |
| T | 切换主要操作栏 | 2252 |
| Ctrl+T | 维护文件日期 | 2284 |
| Ctrl+Shift+T | 切换主要操作栏 | 2252 |
| Ctrl+V | 粘贴剪贴板文件 | 2273 |
| Ctrl+Shift+V | 文件内分页浏览器 | 2310 |
| Num + | 提高显示比例 | 2238 |
| Ctrl+Num + | 提高放大窗倍率 | 2432 |
| Backspace | 浏览上一张 | 2271 |
| Shift+Backspace | 跳到文件前页 | 2250 |
| Delete | 删除所选文件 | 2261 |
| Ctrl+Delete | 删除所选文件 | 2261 |
| Shift+Delete | 删除所选文件 | 2261 |
| Ctrl+Shift+Delete | 删除所选文件 | 2261 |
| Num / | 按物理像素显示 | 2254 |
| Ctrl+Num / | 保持浏览缩放比例 | 2277 |
| Alt+Down | 图片纵向充满视区 | 2301 |
| End | 浏览最后一张 | 2236 |
| Shift+End | 跳到文件末页 | 2251 |
| Esc | 返回目录浏览 | 2326 |
| Shift+Esc | 最小化应用窗口 | 2327 |
| F1 | 显示本地操作帮助 | 2241 |
| Shift+F1 | 拖拽平移工具 | 2371 |
| F2 | 所选项目改名 | 2262 |
| Shift+F2 | 矩形选取工具 | 2372 |
| Shift+F3 | 区域放大工具 | 2373 |
| Ctrl+F4 | 关闭当前查看内容 | 2255 |
| Ctrl+Shift+F4 | 关闭当前查看内容 | 2256 |
| F5 | 重新读取图片 | 2237 |
| Home | 浏览第一张 | 2234 |
| Shift+Home | 跳到文件首页 | 2248 |
| Insert | 选择图片或压缩文件 | 2258 |
| Ctrl+Insert | 复制当前像素 | 2260 |
| Shift+Insert | 粘贴剪贴板文件 | 2273 |
| Ctrl+Alt+Left | 画面左转九十度 | 2328 |
| Num * | 整图适配视区 | 2253 |
| PageDown | 浏览下一张 | 1992 |
| Shift+PageDown | 跳到文件后页 | 2249 |
| Ctrl+Num 0 | 清除星级 | 2114 |
| Num 1 | 浏览最后一张 | 2236 |
| Ctrl+Num 1 | 设置星级1 | 2115 |
| Ctrl+Num 2 | 设置星级2 | 2116 |
| Num 3 | 浏览下一张 | 1992 |
| Ctrl+Num 3 | 设置星级3 | 2117 |
| Ctrl+Num 4 | 设置星级4 | 2118 |
| Ctrl+Num 5 | 设置星级5 | 2119 |
| Num 7 | 浏览第一张 | 2234 |
| Num 9 | 浏览上一张 | 1993 |
| Alt+Num 0 | 清除颜色标签 | 22323 |
| Alt+Num 1 | 设置颜色标签1 | 22324 |
| Alt+Num 2 | 设置颜色标签2 | 22325 |
| Alt+Num 3 | 设置颜色标签3 | 22326 |
| Alt+Num 4 | 设置颜色标签4 | 22327 |
| Alt+Num 5 | 设置颜色标签5 | 22328 |
| Backslash | 切换挑选标记 | 12210 |
| Ctrl+- | 降低放大窗倍率 | 2431 |
| Ctrl+Shift+- | 降低放大窗倍率 | 2431 |
| Ctrl++ | 提高放大窗倍率 | 2432 |
| Ctrl+Shift++ | 提高放大窗倍率 | 2432 |
| PageUp | 浏览上一张 | 1993 |
| Shift+PageUp | 跳到文件前页 | 2250 |
| Enter | 返回目录浏览 | 21947 |
| Alt+Enter | 查看项目属性 | 2351 |
| Shift+Enter | 返回目录浏览 | 21947 |
| Alt+Right | 图片横向充满视区 | 2300 |
| Ctrl+Alt+Right | 画面右转九十度 | 2329 |
| Space | 浏览下一张 | 2269 |
| Shift+Space | 跳到文件后页 | 2249 |
| Num - | 降低显示比例 | 2239 |
| Ctrl+Num - | 降低放大窗倍率 | 2431 |
| Tab | 焦点移到下一属性 | 2324 |
| Shift+Tab | 焦点移到下一属性 | 2324 |
| Ctrl+W | 返回目录浏览 | 2232 |
| Alt+W | 恢复先前壁纸 | 2267 |
| Ctrl+Alt+W | 拉伸图片壁纸 | 21897 |
| Ctrl+Shift+W | 重复图片壁纸 | 2266 |
| Ctrl+Shift+Alt+W | 居中图片壁纸 | 2265 |
| Ctrl+X | 剪切所选文件 | 2281 |
| Alt+X | 从图片收藏集移除 | 2385 |
| Ctrl+Alt+X | 使用关联编辑程序 | 2138 |
| Ctrl+Y | 重做处理步骤 | 2312 |
| Z | 输入显示比例 | 2302 |
| Shift+Z | 选区充满视区 | 2331 |
| Ctrl+Z | 退回上个处理步骤 | 2311 |

## Compare / 比较

| Key / 按键 | RustView command / 功能 | ID |
|---|---|---|
| PageDown | 浏览下一张 | 1992 |
| PageUp | 浏览上一张 | 1993 |
| Delete | 删除所选文件 | 2261 |
| 4 | 比较布局4图 | 2167 |
| Num 4 | 比较布局4图 | 2167 |
| F1 | 显示本地操作帮助 | 2241 |
| O | 切换曝光提示覆盖层 | 22109 |
| 1 | 比较布局1图 | 2164 |
| Num 1 | 比较布局1图 | 2164 |
| L | 保持浏览平移位置 | 2160 |
| Esc | 返回目录浏览 | 2326 |
| Backslash | 切换挑选标记 | 2489 |
| M | 照片标签设置 | 2490 |
| 3 | 比较布局3图 | 2166 |
| Num 3 | 比较布局3图 | 2166 |
| 2 | 比较布局2图 | 2165 |
| Num 2 | 比较布局2图 | 2165 |
| U | 切换曝光提示覆盖层 | 22109 |
| H | 显示RGB直方图 | 2088 |
| Ctrl+Shift+H | 显示RGB直方图 | 2088 |
| P | 查看项目属性 | 2351 |
| Character '/' | 按物理像素显示 | 2254 |
| Num / | 按物理像素显示 | 2254 |
| Character '*' | 整图适配视区 | 2253 |
| Num * | 整图适配视区 | 2253 |
| Alt+Down | 图片纵向充满视区 | 2301 |
| Character '+' | 提高显示比例 | 2238 |
| Character '=' | 提高显示比例 | 2238 |
| Num + | 提高显示比例 | 2238 |
| Ctrl+Num / | 保持浏览缩放比例 | 2277 |
| Character '-' | 降低显示比例 | 2239 |
| Character '_' | 降低显示比例 | 2239 |
| Num - | 降低显示比例 | 2239 |
| Alt+Right | 图片横向充满视区 | 2300 |

## Processing / 处理工作区

| Key / 按键 | RustView command / 功能 | ID |
|---|---|---|
| Ctrl+A | 应用当前工具参数 | 22119 |

