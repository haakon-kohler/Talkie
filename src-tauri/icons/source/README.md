# Icon masters

Every PNG, `.icns` and `.ico` in `src-tauri/icons/` is rendered from these
SVGs. Like the CodeMirror bundle, the rendered files are committed artifacts;
nothing renders them during a build.

| File | What it is |
| --- | --- |
| `mark.svg` | The mark alone, on clear, in the supplied artwork's coordinates. |
| `app-icon.svg` | App icon, 128 px and up: Apple's grid (824 tile inset 100 on 1024, continuous corners, soft drop shadow). Where one arc passes over another, the one underneath is cut back by a seam. |
| `app-icon-small.svg` | The same icon without the seams, for 64 px and under, where a seam would be under a pixel wide and only look muddy. |
| `tray*.svg` | Menu-bar template images at 27 × 36 px, i.e. 18 pt tall at @2x, which is the height the tray scales to. Every straight edge sits on a whole pixel. The gap between the halves is wider than in the artwork, which closes up at this size, and there are no seams. `-recording` adds a dot in the right-hand bowl, `-transcribing` a ring. |

Ink is `#121826`. The tray files are pure black on clear, because
`icon_as_template(true)` lets macOS tint them.

## Regenerating

With `resvg` (`cargo install resvg`), from `src-tauri/icons/`:

```sh
for s in 32 128; do resvg -w $s -h $s source/app-icon$([ $s -lt 128 ] && echo -small).svg ${s}x${s}.png; done
resvg -w 256 -h 256 source/app-icon.svg 128x128@2x.png
resvg -w 512 -h 512 source/app-icon.svg icon.png
for t in tray tray-recording tray-transcribing; do resvg -w 27 -h 36 source/$t.svg $t.png; done
```

For `icon.icns`, render 16, 32 and 64 from `app-icon-small.svg`, render
128–1024 from `app-icon.svg` into an `icon.iconset/`, then run `iconutil -c icns icon.iconset`.
The Windows `Square*Logo.png`, `StoreLogo.png` and `icon.ico` follow the same
split at 128 px. Talkie does not ship on Windows, but Tauri expects the files.
