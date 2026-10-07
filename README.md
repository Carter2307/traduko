# Traduko

A translator that lives on the desktop. Traduko is a small mascot that floats
over your windows; click it and a translator panel opens beside it.
Translation runs on this Mac, with no network.

- **French and English to start with**, as you type, and the languages you
  add: Spanish, German, Italian, Dutch, Russian, Swedish, Ukrainian, Hindi,
  Danish, Finnish, Czech, Hungarian, Indonesian.
- **The language is detected**: write in any language that is installed and
  Traduko works out which one it is.
- **American or British English** for the English side.
- **Two models** for French and English: a light one and a more accurate one.
- **Comes back after a restart**: Traduko opens at login, once you allow it.

## Install

```bash
scripts/install.sh --launch
```

This fetches the models once (about 1.2 GB into
`~/Library/Application Support/Traduko/models`), builds the app, makes
`Traduko.app`, installs it in `~/Applications` and opens it. Run it again after
a change to install the new build.

The first time it opens, Traduko shows three screens: what it does, the model
to download, and the permission to open at login. The last one is left out
when there is nothing to allow. `install.sh` has fetched the models already,
so the second screen finds them; a copy of the app that came without them
downloads them there, from Hugging Face, with the `curl` of macOS.

### More languages

A language is two small models, to English and from it (about 300 MB on
disk, twice that to download). Add one from the panel, with **More
languages…** in either language menu, or from a terminal:

```bash
scripts/fetch-models.sh --lang es,de
```

`scripts/fetch-models.sh --list` gives the codes. Between two languages
that are not English (French to German, Spanish to French) Traduko translates
through English: two translations, so a little slower and a little less
exact. Spanish and German have been run and their installed weights are
pinned by SHA-256; the other languages are pinned as downloaded and record
their installed SHA-256 the first time they are fetched (see the table in
`scripts/fetch-models.sh`).

### Disk space

With both models installed, French and English are only translated by the
one that is chosen: the other one is on disk for nothing (about 300 MB for
the light one, 930 MB for the accurate one). **Delete unused model** in the
Light / Accurate menu removes it; the entry is there when there is something
to remove, and says how much. **Download models…** brings it back. The
languages you added are never removed: each has one model per direction.

To remove everything (dry run first, then `--yes`):

```bash
scripts/uninstall.sh
```

## Use

| Do this | To get this |
|---|---|
| Click Traduko | Open or close the panel |
| Drag Traduko | Move it; the place is remembered |
| Type or paste in the top card | The translation, a moment after you stop |
| The language over the top card | Choose the language to read, or **Detect language** to let Traduko tell (the default) |
| The language over the bottom card | Choose the language to translate to |
| The arrows in the top card, or ⌘⇧S | Swap the languages |
| American / British | Choose the English you want, when translating to English |
| Light / Accurate | Choose the model, Traduko's size (small, medium, large) and its colour (orange, pink, violet, blue, teal), download the other model or delete it, open at login, quit |
| ⌘⇧C, or Copy | Copy the translation |
| Esc, or × | Close the panel |

## How it is made

| Crate | What it holds |
|---|---|
| `crates/app` | The windows (mascot, panel, first screens), in [GPUI](https://www.gpui.rs) |
| `crates/blob` | Traduko's shapes and motion: springs, blinks, hops. No UI dependency |
| `crates/engine` | The models and their download, the sentence pipeline and the worker thread |
| `crates/dialect` | American and British spelling and vocabulary |
| `crates/login` | Open at login, one running copy, bundle paths |

The models are [OPUS-MT](https://github.com/Helsinki-NLP/Opus-MT) from
Helsinki-NLP on Hugging Face (`opus-mt-fr-en`, `opus-mt-en-fr`, their
`tc-big` versions, and `opus-mt-es-en`, `opus-mt-en-es` and so on for the
other languages), run with [candle](https://github.com/huggingface/candle).
They translate one sentence at a time, and they translate idioms word for
word: "il pleut des cordes" becomes "it's raining ropes".

A model knows one direction and lives in a folder named after it
(`light/es-en`). The engine has no list of languages: it uses the folders it
finds, so a model put there by hand works too. The language of a text is
told by the language recognizer of macOS (the NaturalLanguage framework,
offline), asked only for the languages that are installed, and Traduko follows
it only when it leaves little doubt.

American and British English come from a word list
([VarCon](http://wordlist.aspell.net/varcon/)) applied to the English text
after translation; the notices are in `crates/dialect`.

## Develop

```bash
cargo run -p traduko                   # the app, without a bundle
cargo test --workspace                 # every test
cargo run -p traduko-blob --example sheet -- sheet.svg   # Traduko's poses as a picture
cargo run --release -p traduko-engine --example install -- light   # download a set as the app does (or a language: es)
cargo run --release -p traduko-engine --example translate -- --dir es-de "Hola"   # translate from a terminal
cargo run --release -p traduko-blob --example icon -- assets/icon-1024.png
```

Ways to drive the app without the mouse, for captures:
`TRADUKO_DEMO=1` plays every mood, `TRADUKO_PANEL=1` opens the panel without
taking the keyboard, `TRADUKO_TEXT="..."` types into it,
`TRADUKO_APPEARANCE=light|dark` forces a theme, `TRADUKO_CLICK="x,y"` clicks a
point of the panel inside the app, `TRADUKO_FPS=1` prints the mascot's frame
rate, `TRADUKO_SUPPORT_DIR=<folder>` keeps the run away from the real settings,
`TRADUKO_QUIT_AFTER=<seconds>` ends the run.

The first screens come back with settings that never saw them: an empty
`TRADUKO_SUPPORT_DIR`, and an empty `TRADUKO_MODELS_DIR` for the download.
While they are up, `TRADUKO_PANEL` and `TRADUKO_CLICK` drive them in place of
the panel. A development build cannot be a login item, so
`TRADUKO_LOGIN_ITEM=ask|allowed|approval` answers for macOS and brings the
third screen.

The build needs the `runtime_shaders` feature of GPUI (set in
`crates/app/Cargo.toml`) unless Xcode's Metal Toolchain is installed.
