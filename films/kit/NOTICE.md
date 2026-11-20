# Third-party files

The kit is the Fjord explainer kit, copied at its commit 7ce714b533 without its examples/. These files in it
were made by others, and this repository's MIT licence does not cover them. Each built film inlines
all of them.

| File | What | Made by | Licence | Licence text here |
|---|---|---|---|---|
| `kit/vendor/gsap.min.js` | GSAP 3.15.0, the animation library | GreenSock, owned by Webflow | GSAP's standard licence, <https://gsap.com/standard-license>, named in the file's header | No: see below |
| `kit/vendor/MotionPathPlugin.min.js` | GSAP plugin, 3.15.0 | The same | The same | The same |
| `kit/vendor/DrawSVGPlugin.min.js` | GSAP plugin, 3.15.0 | The same | The same | The same |
| `kit/vendor/MorphSVGPlugin.min.js` | GSAP plugin, 3.15.0 | The same | The same | The same |
| `kit/vendor/CustomEase.min.js` | GSAP plugin, 3.15.0 | The same | The same | The same |
| `kit/vendor/icons.js` | Icon paths from lucide-static 1.60.0 | Lucide Icons and Contributors. Some icons come from Feather, by Cole Bemis | ISC, and MIT for the icons from Feather | [`licences/lucide.txt`](licences/lucide.txt) |
| `kit/fonts/geist-regular.otf` | Geist, regular | The Geist Project Authors (Vercel) | SIL Open Font License 1.1 | [`licences/geist.txt`](licences/geist.txt) |
| `kit/fonts/geist-medium.otf` | Geist, medium | The same | The same | The same |
| `kit/fonts/geist-mono.ttf` | Geist Mono | The same | The same | The same |

`licences/lucide.txt` is the `LICENSE` file of the package lucide-static 1.60.0. `licences/geist.txt`
is `OFL.txt` of <https://github.com/vercel/geist-font> at its tag 1.4.2, which carries the same
copyright line as the fonts here (2024). Each built film ends with both texts, in a comment
(`tools/build.py`).

GSAP stays Webflow's property. Its licence (last modified 30 May 2025) allows using GSAP "on any
website, web application, or digital interface", which is what the films do. It asks for no copy of
its text, only that the notices in the files stay, and they are unchanged here. It does not say
whether the files may be kept in a public repository.

The spoken sentences under each film's `voice/` were made on this Mac by the Kokoro-82M speech
model (`mlx-community/Kokoro-82M-bf16`), voice Heart (`af_heart`). The model is not in this
repository.
