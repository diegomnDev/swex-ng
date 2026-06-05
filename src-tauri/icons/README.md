# Icons

Tauri needs app icons to **bundle** (`pnpm tauri build`). They are not required
for `pnpm tauri dev`.

Generate them from any square PNG (1024×1024 recommended):

```bash
pnpm tauri icon path/to/logo.png
```

This populates `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`
(macOS) and `icon.ico` automatically.
