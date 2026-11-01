# Notices

Minecraft Rust Seedmap links the world generator of **Minecraft Rust**
(<https://github.com/elloexe/minecraft-rust>, a copy of Claudium): its
`claudium-world` and `claudium-core` crates, pinned in `Cargo.toml`. The
released executable contains that code, distributed under its MIT licence:

```text
MIT License

Copyright (c) 2026 Ludovic Marie and the Horizon contributors

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
```

The interface font is **Inter** by Rasmus Andersson
(<https://rsms.me/inter/>), under the SIL Open Font License 1.1; see
`assets/fonts/Inter-LICENSE.txt`.

The executable embeds world generation data (noise settings, density
functions, biome tags, structure definitions and the multi-noise biome
parameter tables) extracted at build time from the official Minecraft: Java
Edition 26.3 server jar, downloaded from Mojang. No Mojang files are stored in
this repository.

Not an official Minecraft product. Not approved by or associated with Mojang
or Microsoft.
