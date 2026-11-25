# Desktop icons

Placeholder icons generated from `public/icon-512.png` (the OpenMesh hex mesh mark).

- `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.png` — used by Tauri for Linux/macOS.
- `icon.ico` — Windows resource.
- macOS `.icns` is intentionally absent; regenerate the full platform set with:

```bash
pnpm dlx @tauri-apps/cli@2 icon ../../public/icon-512.png
```

Tauri desktop packaging requires WebKitGTK development headers; see the parent README.
