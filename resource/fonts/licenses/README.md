# Word cloud fonts

The fonts in `resource/fonts/` are the faces offered under Settings ▸ Articles ▸ Word cloud font.
They come from the [google/fonts](https://github.com/google/fonts) repository at commit
`e7db6ca603fd4862010383ff17ffd6cc4ffac1c0`, unmodified. Each font's license is the file in this
folder named after it.

| File | Family | Source | License |
|---|---|---|---|
| `AbrilFatface-Regular.ttf` | Abril Fatface | `ofl/abrilfatface` | SIL Open Font License 1.1 (`abrilfatface-OFL.txt`) |
| `AmaticSC-Bold.ttf` | Amatic SC | `ofl/amaticsc` | SIL Open Font License 1.1 (`amaticsc-OFL.txt`) |
| `Bungee-Regular.ttf` | Bungee | `ofl/bungee` | SIL Open Font License 1.1 (`bungee-OFL.txt`) |
| `Chewy-Regular.ttf` | Chewy | `apache/chewy` | Apache License 2.0 (`chewy-LICENSE.txt`) |
| `Lobster-Regular.ttf` | Lobster | `ofl/lobster` | SIL Open Font License 1.1 (`lobster-OFL.txt`) |
| `Pacifico-Regular.ttf` | Pacifico | `ofl/pacifico` | SIL Open Font License 1.1 (`pacifico-OFL.txt`) |
| `PermanentMarker-Regular.ttf` | Permanent Marker | `apache/permanentmarker` | Apache License 2.0 (`permanentmarker-LICENSE.txt`) |
| `Righteous-Regular.ttf` | Righteous | `ofl/righteous` | SIL Open Font License 1.1 (`righteous-OFL.txt`) |
| `SpecialElite-Regular.ttf` | Special Elite | `apache/specialelite` | Apache License 2.0 (`specialelite-LICENSE.txt`) |

The licenses live in this subfolder because `day build` bundles every file directly in
`resource/fonts/` and accepts only `.ttf` and `.otf` files there.
