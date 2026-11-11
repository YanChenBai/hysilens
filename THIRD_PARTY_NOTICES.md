# Third-party notices

The local QMC1 preprocessing in `crates/hysilens-core/src/platforms/qq_music/lyrics.rs` is adapted from
[jixunmoe-go/qrc](https://github.com/jixunmoe-go/qrc), `internal/qmc/qmc.go` and
`internal/qrc/qrc.go`.

MIT License

Copyright (c) 2023 Jixun Wu

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

The `lyrics-core`, `lyrics-crypto` and `lyrics-parsers` dependencies are distributed
under Apache-2.0; their package distributions include their license notices.

The read-only WeSing memory capture in crates/hysilens-core/src/platforms/wesing is adapted from
[mywltd/WesingCap](https://github.com/mywltd/WesingCap), lyric/finder.go,
lyric/reader.go, lyric/timer.go and proc/memory.go.

MIT License

Copyright (c) 2026 VTB-LINK

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## AF Media Bar — KuGou read-only playback capture

The module offsets, pointer chain, process selection and checked memory reads in
`crates/hysilens-core/src/platforms/kugou/memory.rs` are adapted from
[Fervent-Tempo/AF-Media-Bar](https://github.com/Fervent-Tempo/AF-Media-Bar),
`src/AFMediaBar/Classes/Services/Media/Sources/KuGou/KuGou.cs` and
`src/AFMediaBar/Classes/Services/Win32/Memory.cs`, revision
`1420ffb7993634ea7bfe7ae3858a90f65b778f3f`.
The preferred duration chain for KuGou 20.1.61.28014 also follows `KuGou.cs`
at revision `8e0564f91e10541eed321681ddf72da024e3fa6f` of the same project.
The KuGou implementation was contributed by Miaoyww in
[PR #83](https://github.com/Fervent-Tempo/AF-Media-Bar/pull/83).
The Rust adaptation reads process memory only; it does not inject code or modify the player.

MIT License

Copyright (c) 2026 AmorFate

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

# Bundled lyric fonts

The scroll plugin bundles Noto Sans SC (Copyright 2014-2021 Adobe, with Reserved Font Name 'Source'), from [Google Fonts](https://github.com/google/fonts/tree/main/ofl/notosanssc), under [SIL OFL 1.1](plugins/scroll/public/fonts/NotoSansSC-OFL.txt).

The tilted plugin bundles ZCOOL XiaoWei (Copyright 2018 The ZCOOL XiaoWei Project Authors), from [Google Fonts](https://github.com/google/fonts/tree/main/ofl/zcoolxiaowei), under [SIL OFL 1.1](plugins/tilted/public/fonts/ZCOOLXiaoWei-OFL.txt).

These are unmodified upstream font files downloaded 2026-11-11. Full copyright and license notices ship in each plugin's `fonts/` directory. OFL permits commercial use and bundling with software under its terms; font files may not be sold by themselves. The project license does not replace the font licenses.
