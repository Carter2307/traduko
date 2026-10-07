# Third-party work in Coco

| What | From | Licence | Where |
|---|---|---|---|
| Translation models `opus-mt-fr-en`, `opus-mt-en-fr`, `opus-mt-tc-big-fr-en`, `opus-mt-tc-big-en-fr` | [OPUS-MT](https://github.com/Helsinki-NLP/Opus-MT), Helsinki-NLP (Jörg Tiedemann and others), trained on [OPUS](https://opus.nlpl.eu) data | CC-BY 4.0 (the base pair is also tagged Apache-2.0 on Hugging Face) | fetched by `scripts/fetch-models.sh` or by the app on its first run, not in this repository |
| American and British word lists | [VarCon](http://wordlist.aspell.net/varcon/) 2020.12.07, Kevin Atkinson and Benjamin Titze, processed and corrected | VarCon and Ispell notices | `crates/dialect/LICENSE-VARCON`, `crates/dialect/THIRD_PARTY_NOTICES.txt` |
| Fonts Geist and Geist Mono | [The Geist Project Authors](https://github.com/vercel/geist-font) | SIL Open Font License 1.1 | `assets/fonts/LICENSE-Geist.txt` |
| GPUI | Zed Industries, as republished in the `gpui-pre` crates | Apache-2.0 | Cargo dependency |
| GPUI Component and its icon set | Longbridge; icons from [Lucide](https://lucide.dev) | Apache-2.0; ISC for the icons | Cargo dependency |
| candle | Hugging Face | MIT OR Apache-2.0 | Cargo dependency |

The mascot's look follows reference images given by the app's owner; the
shapes and the motion are drawn by this project's own code.
