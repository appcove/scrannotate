# MSIX tile & logo assets

`build-msix.ps1` copies this folder into the package. The Microsoft Store
requires these PNGs (transparent background where noted). Drop them here;
they are referenced by `../AppxManifest.xml`.

| File | Size (px) | Used for |
|------|-----------|----------|
| `Square44x44Logo.png` | 44×44 | taskbar / app list icon |
| `Square150x150Logo.png` | 150×150 | medium Start tile |
| `Wide310x150Logo.png` | 310×150 | wide Start tile |
| `StoreLogo.png` | 50×50 | Store listing / Properties |

Best practice is to generate the full scaled asset set (targetsize + scale
variants) with the **Manifest Designer** in Visual Studio, or the
`MakePri`/asset-generator tooling, from a single 400×400+ source logo.
scrannotate currently ships **no icon** — creating one is a prerequisite for
Store submission (see `../../../docs/SIGNING.md`).
