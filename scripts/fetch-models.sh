#!/bin/bash
# fetch-models.sh: download the translation models and put them where Traduko
# reads them.
#
#   ./scripts/fetch-models.sh                French and English, both sets (same as --all)
#   ./scripts/fetch-models.sh --light        the small French and English models only
#   ./scripts/fetch-models.sh --accurate     the large ones only
#   ./scripts/fetch-models.sh --lang es,de   Spanish and German, to and from English
#   ./scripts/fetch-models.sh --list         the languages that --lang takes
#
# The options add up: --light --lang es fetches the small set and Spanish.
#
# Destination: $TRADUKO_MODELS_DIR, or ~/Library/Application Support/Traduko/models
#
#   <models>/light/fr-en      Helsinki-NLP/opus-mt-fr-en          about 150 MB each
#   <models>/light/en-fr      Helsinki-NLP/opus-mt-en-fr
#   <models>/accurate/fr-en   Helsinki-NLP/opus-mt-tc-big-fr-en   about 460 MB each
#   <models>/accurate/en-fr   Helsinki-NLP/opus-mt-tc-big-en-fr
#   <models>/light/es-en      Helsinki-NLP/opus-mt-es-en          a language of --lang:
#   <models>/light/en-es      Helsinki-NLP/opus-mt-en-es          about 150 MB each way
#
# A language comes as two small models, to English and from it. Traduko finds
# them by their folder, and translates between two languages that have no
# model of their own (Spanish to German) through English.
#
# Every file is taken at a pinned commit, and the weights are checked against
# a known SHA-256 twice: as downloaded, and as installed. Only safetensors
# weights are fetched: never a pickle (.bin), which can run code when it is
# loaded.
#
# The small models are published in 32-bit floats. They are installed in 16
# bits, half the size, by the `to-f16` example of the engine crate, so this
# script needs cargo for them. The large models are 16-bit already.
#
# Files already in the Hugging Face cache (~/.cache/huggingface) are not
# downloaded again, and on APFS they are cloned, not copied. Weights that are
# installed and intact are left alone: a second run takes a few seconds.
set -euo pipefail
cd "$(dirname "$0")/.."

MODELS_DIR="${TRADUKO_MODELS_DIR:-$HOME/Library/Application Support/Traduko/models}"
SMALL_FILES="config.json source.spm target.spm vocab.json"

# One line per model: set, direction, repository, commit, then the SHA-256
# of model.safetensors as downloaded and as installed. The two differ for the
# small models, which are converted. The conversion gives the same bytes on
# every run; if the converter or its dependencies change and the result with
# them, check a translation and update the last column.
#
# A dash in the last column is a model that had not been installed anywhere
# when its line was written. Its download is checked like the others; what
# it becomes once converted is written next to it, in
# model.safetensors.sha256, and checked against that from then on. To pin
# it, install it, check a translation, and put that SHA-256 here and in
# crates/engine/src/install.rs, which holds the same table for the app.
#
# The Hub has most of the small models as safetensors only in a pull request
# of its conversion bot (SFconvertbot): the commit is then the head of the
# latest one. For opus-mt-en-fr that is pull request 9.
#
# The French and English sets come first; then the languages of --lang, each
# to English and from it.
MODELS="
light fr-en Helsinki-NLP/opus-mt-fr-en c4aed37b318c763fd177aa449b44e3b783cc6c02 6e3837f34b903802c3d0d670362b997cee6e87584a1108eb3fa89e4625e4424a 44622407d10fd34e5c98f532b70bb635dceb6f92af1d6600419cad9986753844
light en-fr Helsinki-NLP/opus-mt-en-fr c96fcd7f38a4c0d1ac9b61e255b47f3f7ff55e5e c32d3003ced798c78b9ca90fb69c8e23ad5b4c64c500628bdf3303bbc9272816 1b66c1c9c0baa7cca82e52a3711662da0d5f2e103bd60c810f4198dc94d44158
accurate fr-en Helsinki-NLP/opus-mt-tc-big-fr-en 5fa3b3c9fa3bcd65fbf31206c3ec7419c9d9cd7d 1e9d73c1af19660aad2e0733e12f8d06745eb54c77f48bae4831e377ccaf6d6e 1e9d73c1af19660aad2e0733e12f8d06745eb54c77f48bae4831e377ccaf6d6e
accurate en-fr Helsinki-NLP/opus-mt-tc-big-en-fr 6e062862ced6f5622a589ab2aadc2a1d4978db78 5c88b4f7a63934b8be72372b86661e616227c20be4b75612eeee7dca96494217 5c88b4f7a63934b8be72372b86661e616227c20be4b75612eeee7dca96494217
light es-en Helsinki-NLP/opus-mt-es-en 725b7965a8cac11ebe80ea671e72e0b7e8b28a9f 07d9fc8881ac9bc8f06fbe3576ca16045c684c7d529e9733cbeeaaf2c78f9539 a1089b29a562c210e4c0fb16283ceca39e810f5ecd75013f5138efc7d50a1852
light en-es Helsinki-NLP/opus-mt-en-es fdaddf76f50fcc1583ba42f95965862a7ab30f97 b3ecbf954573c2fd95d05d3ad4618baf961793db0da07c2add2e8a3a6cd78d0b 2ecae2e77bbe980b5c69277ff4c7a7474e63c392ea89a0bc01b907033843d6d0
light de-en Helsinki-NLP/opus-mt-de-en 6f6b23ef5ef8a586414ad9d2b7dda64ca5352935 5b1a66c79f6e871eb01b8818819e29bbdb6a25139af210b97b6efbf862493b4d 7c7e35679866fffc0ed799fb6c559956564e331053f747e23fed53418526532b
light en-de Helsinki-NLP/opus-mt-en-de 0012d6d6ff4b4dd06bee042d3294295f0e587abd 3fb78700bce624990eff6468d0e14fbde18a6aedac35821fa8c0c4ead5fae014 4d47cc7827f85586b4d04f7629069ad61b49d0efb1d97fe2e42f48d684c3aca4
light it-en Helsinki-NLP/opus-mt-it-en d43c8316fd1645309b94b88ec52b13e505cc91d9 a6e1dd4180f8aeec864ea5d79678025fa013e93df935d46159ebd6eb164782c1 -
light en-it Helsinki-NLP/opus-mt-en-it 5483bd70fe6e934c56875647d5dbad58f09bbd16 7012b826195f82a68efc878b39e763d2fbf8ebb485811375e9d9632899eba7d0 -
light nl-en Helsinki-NLP/opus-mt-nl-en 385c15b2b304774437eac7c2b584e4868ca1c6f0 048b19007be3c1f438566de765b8803035415d295706b9bfd9df7e083f6154db -
light en-nl Helsinki-NLP/opus-mt-en-nl cb106147f56946f00045d2f9768539949e660489 1e227db75d9134498000e1e91c9c8f6f6347d586d5fa2b7b08ea2c4e630c24db -
light ru-en Helsinki-NLP/opus-mt-ru-en 7c84e70e05294db4fead6135d04585020413f78b f73dc54675dc0da9ca6704098cadae3562f149b1f93c585f9d21c8502a760421 -
light en-ru Helsinki-NLP/opus-mt-en-ru 3ff4dbd98515ba253f08f183e0de0b7ccf189ee3 07f5d90a45955a73412a118f0635315a28ea5a5d82522f3be008e71d452526c0 -
light sv-en Helsinki-NLP/opus-mt-sv-en a66266b25c226b4666d58f8b8c1ee62a74386cfc 482ca42946152a8c0af2f5ae7093823e5682e50af017ed7ab9d67f53a1ce6903 -
light en-sv Helsinki-NLP/opus-mt-en-sv 4e83c9f90366cb438892040c46c66aa3b8406981 1f10f4b66c86e4d98cdfa01e842ed2735312a6a99da08978b64e579e875fb0cb -
light uk-en Helsinki-NLP/opus-mt-uk-en 5c5e4e6e58d47ea2d90b657a7b0d270d06a89be2 a52221e3b5ad186534f7c5a1af9b89728db9ce437a08cb27c8c5d08cc13014fc -
light en-uk Helsinki-NLP/opus-mt-en-uk 55104bebd23cd2d87761e650aa6ac7369b379d1c a0f3dce3522dc5c991df5b5b669c38622234f56eeb048f9ad95e4459e0ee1088 -
light hi-en Helsinki-NLP/opus-mt-hi-en 338f1e1226738d9095c1e8b6f8931c46c0de85a4 f09b34ffd2a8d8046b1733f0796b390c2be907effe7c6a70ed162fae9aae68c1 -
light en-hi Helsinki-NLP/opus-mt-en-hi 43211ff5af2aad1a5da49a7017bf790346dc0eef 46ae1116913bce01c9d848a78f62da2bd986d728bced4dc1acd5fedf4338ac5e -
light da-en Helsinki-NLP/opus-mt-da-en 0dd912927fa4b60317a44ed04c9ffd4c56d4b4ee a8effdcf176755cc436f107719db0b082d9156c9ae54e61dd426f2f3b4f36243 -
light en-da Helsinki-NLP/opus-mt-en-da bee92961eb97c1a70d247637ab527439ed915b14 af93346239b6935d940ea0d00aff951915d7732027644f1cf9cb5f17e1adf5a4 -
light fi-en Helsinki-NLP/opus-mt-fi-en 22095576b4b6a72271c6ad7bcb32b91066e7ca94 2f8b591f7c7543ecf557653e8b90900478fc134adc88df62404b30cf95958c28 -
light en-fi Helsinki-NLP/opus-mt-en-fi f294bfca8f7be502fde2362e4d0ac09fb232415b 2a43d7e0416faf10acb80a106d470f02791f0d6be7a36bcc011cd67943fd07d2 -
light cs-en Helsinki-NLP/opus-mt-cs-en 0d5030270f2d0bf4456b821b77dce67f9367e9fd 28fcc2e7d7cfcea474fe35fe9ac0471dcfa165d5716db5189a19b8035c6e48aa -
light en-cs Helsinki-NLP/opus-mt-en-cs cfeb086a3a864c78e3e32da25bcad89bcb4a0359 79f82c7a1228c0e56cd014af4f8712405908df1bd9099e0d873f0bf3b6374204 -
light hu-en Helsinki-NLP/opus-mt-hu-en 8e850310a8d25c50c1be55f42b707a770ade49a6 65104eb75a197ffe96d802fc8203f09eff681040e69713df75b7532d32384552 -
light en-hu Helsinki-NLP/opus-mt-en-hu dfa8f0482de57e05614fac8f93d83665cfa846a7 1bc13ce06cb34e227cecbc241ab3eac2204cb8a7faffae379ceb1e6dff888fb9 -
light id-en Helsinki-NLP/opus-mt-id-en b7a06a6a2bb269a4de8cdad71940035eae97cea7 e71532a9cfa6392e7ac5f725d3c9dc82ff6c5a9701b1a407db6dcc25bf4440ce -
light en-id Helsinki-NLP/opus-mt-en-id 916d091c1efaba75c4ba7cd2f544ef2fbcc1a194 e7169d62592b1a9fefe33b7ab410c5d1d619e39b1df3d07175bd57d8331ffcd1 -
"

usage() {
  sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
}

# "es" for es-en and for en-es: the side that is not English.
language_of() {
  local language="${1#en-}"
  echo "${language%-en}"
}

# The languages of the table, French aside: it comes with the sets.
known_languages() {
  local set_name direction rest language known=""
  while read -r set_name direction rest; do
    [ -n "$set_name" ] || continue
    language="$(language_of "$direction")"
    case " fr $known " in *" $language "*) ;; *) known="$known $language" ;; esac
  done <<TABLE
$MODELS
TABLE
  echo "${known# }"
}

want_light=0
want_accurate=0
want_languages=""
[ $# -eq 0 ] && set -- --all
while [ $# -gt 0 ]; do
  case "$1" in
    --light) want_light=1 ;;
    --accurate) want_accurate=1 ;;
    --all) want_light=1; want_accurate=1 ;;
    --lang)
      [ $# -ge 2 ] || { echo "fetch-models: --lang needs languages, like --lang es,de" >&2; exit 2; }
      want_languages="$want_languages ${2//,/ }"
      shift ;;
    --lang=*) want_languages="$want_languages $(echo "${1#--lang=}" | tr ',' ' ')" ;;
    --list) known_languages; exit 0 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "fetch-models: unknown option $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

known="$(known_languages)"
for language in $want_languages; do
  case " $known " in
    *" $language "*) ;;
    *) echo "fetch-models: no models for the language $language (there are: $known)" >&2; exit 2 ;;
  esac
done

command -v hf >/dev/null || { echo "fetch-models: the hf command is missing (pip install huggingface_hub)" >&2; exit 1; }

sha256_of() {
  shasum -a 256 "$1" | cut -d ' ' -f 1
}

# Stops unless file $1 has the SHA-256 $2. $3 says which file it is.
verify() {
  local actual
  actual="$(sha256_of "$1")"
  if [ "$actual" != "$2" ]; then
    echo "fetch-models: wrong SHA-256 for $3" >&2
    echo "  expected $2" >&2
    echo "  got      $actual" >&2
    exit 1
  fi
}

install_model() {
  local set_name="$1" direction="$2" repository="$3" commit="$4" downloaded="$5" installed="$6"
  local destination="$MODELS_DIR/$set_name/$direction"
  local weights="$destination/model.safetensors"
  local record="$weights.sha256"
  local snapshot file

  echo "==> $set_name/$direction: $repository@${commit:0:8}"
  mkdir -p "$destination"

  # Without a pinned SHA-256, the one that was recorded at the first
  # installation stands in for it.
  local expected="$installed"
  if [ "$installed" = - ] && [ -f "$record" ]; then
    expected="$(cat "$record")"
  fi

  # A file gets its final name only when it is whole: it is written as
  # .part first, then renamed. cp -c clones on APFS and falls back to a
  # plain copy elsewhere.
  if [ -f "$weights" ] && [ "$(sha256_of "$weights")" = "$expected" ]; then
    echo "    weights already installed and intact"
  else
    snapshot="$(hf download "$repository" model.safetensors --revision "$commit" --quiet | tail -n 1)"
    verify "$snapshot" "$downloaded" "$repository model.safetensors"
    if [ "$set_name" = light ]; then
      command -v cargo >/dev/null || { echo "fetch-models: cargo is missing, and the small models need it" >&2; exit 1; }
      cargo run --release --quiet -p traduko-engine --example to-f16 -- "$snapshot" "$weights.part" | sed 's/^/    /'
    else
      cp -c "$snapshot" "$weights.part"
    fi
    if [ "$installed" = - ]; then
      sha256_of "$weights.part" > "$record.part"
      mv -f "$record.part" "$record"
      echo "    not pinned yet: installed with SHA-256 $(cat "$record")"
    else
      verify "$weights.part" "$installed" "the weights to install in $destination"
    fi
    mv -f "$weights.part" "$weights"
    echo "    weights verified and installed"
  fi

  # With several files, hf prints the folder of the snapshot that holds them.
  # shellcheck disable=SC2086
  snapshot="$(hf download "$repository" $SMALL_FILES --revision "$commit" --quiet | tail -n 1)"
  for file in $SMALL_FILES; do
    [ -f "$snapshot/$file" ] || { echo "fetch-models: $file is missing from $snapshot" >&2; exit 1; }
    cp -c "$snapshot/$file" "$destination/$file.part"
    mv -f "$destination/$file.part" "$destination/$file"
  done
}

while read -r set_name direction repository commit downloaded installed; do
  [ -n "$set_name" ] || continue
  case "$direction" in
    fr-en|en-fr)
      if [ "$set_name" = light ] && [ "$want_light" = 0 ]; then continue; fi
      if [ "$set_name" = accurate ] && [ "$want_accurate" = 0 ]; then continue; fi
      ;;
    *)
      case " $want_languages " in *" $(language_of "$direction") "*) ;; *) continue ;; esac
      ;;
  esac
  # hf and cargo must not read the table that this loop is reading.
  install_model "$set_name" "$direction" "$repository" "$commit" "$downloaded" "$installed" </dev/null
done <<TABLE
$MODELS
TABLE

echo "==> Installed in $MODELS_DIR"
for set_name in light accurate; do
  for weights in "$MODELS_DIR/$set_name"/*/model.safetensors; do
    [ -f "$weights" ] || continue
    folder="$(dirname "$weights")"
    printf '    %-9s %-6s %s\n' "$set_name" "$(basename "$folder")" "$(du -sh "$folder" | cut -f 1)"
  done
done
printf '    %-16s %s\n' "total" "$(du -sh "$MODELS_DIR" | cut -f 1)"
