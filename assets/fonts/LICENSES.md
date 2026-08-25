# Bundled font licenses

| Font | Family name | License | Source |
|---|---|---|---|
| Excalifont Regular | `Excalifont` | MIT (per https://plus.excalidraw.com/excalifont "Download available under MIT licence"; excalidraw discussion #11623 additionally cites OFL-1.1 — both are redistribution-friendly) | https://excalidraw.nyc3.cdn.digitaloceanspaces.com/fonts/Excalifont-Regular.woff2 (decompressed to TTF via fontTools) |
| Nunito Regular (instanced from `Nunito[wght].ttf` at wght=400) | `Nunito` | SIL OFL 1.1 — see `Nunito-OFL.txt` | https://github.com/google/fonts/tree/main/ofl/nunito |
| Comic Shanns v2 | `Comic Shanns` | MIT — see `ComicShanns-LICENSE` | https://github.com/shannpersand/comic-shanns (`v2/comic shanns 2.ttf`) |

Excalidraw `fontFamily` numeric ids map: 1 → Excalifont, 2 → Nunito, 3 → Comic Shanns
(see `xc-core/src/text.rs::family_for`).

If you redistribute this project, keep this file and the license files alongside
the fonts. OFL requires fonts (and derivatives) ship with the OFL text; modified
versions must not use reserved font names where applicable.
