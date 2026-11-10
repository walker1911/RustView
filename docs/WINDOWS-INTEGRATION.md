# Windows 文件关联与压缩包右键 / Windows integration

这份本地试用包提供可选的注册工具。普通看图不需要注册；脚本仅写当前用户的 RustView 关联信息，不要求管理员权限。

## 使用方法

1. 先把完整离线包放到准备长期使用的位置。
2. 双击包根目录的 **`Setup-File-Associations.cmd`**。
3. 工具注册当前支持的图片格式、相机 RAW、PSD/PSB，以及 ZIP／RAR／7z，并打开 Windows 的“默认应用”。在其中选择 RustView，确认你想交给它的格式。Windows 10 的设置页可能只显示默认应用首页，可按扩展名搜索或从资源管理器的“打开方式 → 选择其他应用”设置。
4. 在 ZIP／RAR／7z 文件上右键，选择 **Open with RustView**。Windows 11 上该传统菜单项可能位于 **“显示更多选项”** 内。

图片双击的最终默认选择由 Windows 设置确认。脚本不改写受保护的 UserChoice。默认应用的批量图片清单不包含 ZIP／RAR／7z，这三种只增加右键和“打开方式”候选，不修改压缩软件的默认关联；只想右键看包内图片，可以注册后直接关闭默认应用设置页。

注册使用双引号包裹程序路径和文件参数，支持空格、中文和 `&` 等路径字符。三个压缩包入口都直接把文件路径交给 RustView，不启动命令行解释器读取压缩包内容。已有密码提示仍由查看器处理，不把密码保存在注册表。

## 卸载与移动文件夹

- 双击同一包内的 **`Remove-File-Associations.cmd`**，移除 RustView 的候选关联和三个压缩包菜单项。其他软件的候选项和菜单项保留。
- 如果已在系统中选择 RustView 为默认程序，卸载注册后可到 Windows 默认应用选择替代程序；工具不猜测你之前的默认应用。
- 移动文件夹前可先卸载注册，移动后重新运行 Setup。已经移动也可以直接在新位置重新 Setup，更新程序的绝对路径。
- 多份便携包共享 RustView 这个注册名，最后一次 Setup 的位置生效。旧位置的 Remove 会拒绝删除另一份正在使用的注册。
- 若组织策略禁止脚本或默认应用修改，按系统提示处理；工具不更改全局执行策略。

技术验证：`tools/test-windows-associations.ps1` 在随机临时 HKCU 子树中测试完整安装、重复安装、删除、所有权、命令引号及其他应用保护，结束清理测试子树。它不修改真实文件关联。

## English

Keep the complete portable package in its intended location, then run **Setup-File-Associations.cmd**. It registers RustView as a candidate for its supported images, camera RAW, PSD/PSB and ZIP/RAR/7z under the current user, and opens Windows Default Apps. Confirm your preferred defaults there. Windows 10/11 require that final user choice; the tool does not overwrite protected UserChoice values or take over your archive application's defaults.

The **Open with RustView** verb is available on ZIP/RAR/7z files, possibly under **Show more options** on Windows 11. Run **Remove-File-Associations.cmd** to remove RustView's registrations. After moving the portable folder, run Setup again. The last registered portable copy owns the shared RustView registration; an older copy cannot uninstall a newer copy's registration.

Registration is optional and user-scoped. It uses the executable from this package, quotes both executable and file paths, and stores no archive passwords. Native first-level Windows 11 context-menu extensions are not included.

## Microsoft references

- [Application registration, Applications and SystemFileAssociations](https://learn.microsoft.com/en-us/windows/win32/shell/app-registration)
- [Default Programs capabilities and ProgIDs](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs)
- [Launch Default Apps for a per-user registered application](https://learn.microsoft.com/en-us/windows/apps/develop/launch/launch-default-apps-settings)
