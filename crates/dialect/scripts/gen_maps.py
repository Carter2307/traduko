#!/usr/bin/env python3
"""Generate the compact spelling maps embedded in the `dialect` crate.

Input : data/varcon/varcon.txt   VarCon 2020.12.07 (Kevin Atkinson / Benjamin Titze), Latin-1
        data/extra_spelling.tsv  hand-written additions and deletions (see that file)
Output: assets/spelling_b2a.tsv  british<TAB>american
        assets/spelling_a2b.tsv  american<TAB>british_ise[<TAB>british_oxford]
                                 (3rd column only when it differs; "=" = same as american)
        data/report_ambiguous.txt  pairs dropped because the source spelling is also a
                                   correct word of the target variant (check, tire, ...)
        data/report_untrusted.txt  pairs dropped because they come from an unverified
                                   VarCon cluster and do not follow a known pattern

How a pair is selected
----------------------
* A VarCon line gives, per spelling, the variants it belongs to: A (American), B (British
  "-ise"), Z (British "-ize" / Oxford), each either preferred ("A"), equal ("A.") or a
  lesser variant ("Av", "AV", "A-", "Ax").  We only pair PREFERRED American with
  PREFERRED British spellings, and never rewrite a spelling that is preferred-or-equal
  in the target variant ON THE SAME LINE ("axe", "among", "inquire" stay as they are).
* Ambiguity filter: a pair x -> y is dropped when x is also preferred-or-equal in the
  target variant on ANY other (non-rare) line, i.e. when x is a real word of the target
  variant with another meaning: check/cheque, tire/tyre, program/programme, story/storey,
  curb/kerb, meter/metre, practice/practise, license/licence, draft/draught, prize/prise.
  The Rust crate handles the frequent ones with context rules (assets/rules_*.tsv).
* Trust filter: VarCon only hand-verified its common clusters.  Unverified clusters of
  SCOWL level >= 70 contain machine-made junk ("et / aet", "penne / pennae", "ern / ren"),
  so from those we keep a pair only if it is explained by trusted pairs and patterns
  (-ize/-ise, -yze/-yse, a trusted pair inside a longer word, Greek ae/oe stems).

Python standard library only.  Run:  python3 scripts/gen_maps.py
"""
import os
import re
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VARCON = os.path.join(ROOT, "data", "varcon", "varcon.txt")
EXTRA = os.path.join(ROOT, "data", "extra_spelling.tsv")
OUT_DIR = os.path.join(ROOT, "assets")
TRUSTED_UNVERIFIED_LEVEL = 60     # unverified clusters up to this SCOWL level are taken as is
MIN_INNER = 5                     # shortest trusted word accepted inside a longer word

TAG_RE = re.compile(r"^([ABZCD_])([.vVx-]?)$")
WORD_RE = re.compile(r"^[a-z]+$")

# American -> British substitutions accepted inside words of unverified clusters.
PATTERNS = [
    ("hem", "haem"), ("emi", "aemi"), ("esthe", "aesthe"), ("pale", "palae"),
    ("rrhe", "rrhoe"), ("ped", "paed"), ("gynec", "gynaec"), ("estr", "oestr"),
    ("edema", "oedema"), ("celi", "coeli"), ("etio", "aetio"),
]
# -ize / -yze verb morphology, anchored at the end of the word, stem of 3+ letters.
IZE_RE = re.compile(
    r"^([a-z]{3,})(i|y)z(e|es|ed|er|ers|ing|ings|ingly|edly|ation|ations|ational|able|ability|ement|ements)$"
)
MAX_UNVERIFIED_LEVEL = 80         # 95 / 99 = "may not even be a legal word"


def parse(path):
    """Yield (level, verified, rare, entries) for every data line.
    entries = [(word, {category: marker})]; marker '' = preferred, '.' = equal."""
    level, verified = 0, False
    with open(path, encoding="iso-8859-1") as fh:   # varcon.txt is Latin-1
        for raw in fh:
            line = raw.rstrip("\n")
            if not line.strip() or line.startswith("##"):
                continue
            if line.startswith("#"):
                m = re.search(r"\(level (\d+)\)", line)
                level = int(m.group(1)) if m else 0
                verified = "<verified>" in line
                continue
            line = line.split(" # ")[0]
            parts = line.split(" | ")
            head, notes = parts[0], parts[1:]
            rare = any(n.strip().startswith("(-)") for n in notes)
            entries = []
            for chunk in head.split(" / "):
                if ": " not in chunk:
                    continue
                tags, word = chunk.split(": ", 1)
                cats = {}
                for t in tags.split():
                    m = TAG_RE.match(t)
                    if m:
                        cats[m.group(1)] = m.group(2)
                entries.append((word.strip(), cats))
            if entries:
                yield level, verified, rare, entries


def pref(entries, cat):
    return [w for w, c in entries if c.get(cat) == ""]


def ok(entries, cat):
    return [w for w, c in entries if c.get(cat) in ("", ".")]


def explained(a, b, trusted):
    """True if British `b` derives from American `a` by <= 2 trusted substitutions."""
    def step(word):
        out = set()
        m = IZE_RE.match(word)
        if m:
            out.add(f"{m.group(1)}{m.group(2)}s{m.group(3)}")
        for src, dst in PATTERNS:
            i = word.find(src)
            while i != -1:
                out.add(word[:i] + dst + word[i + len(src):])
                i = word.find(src, i + 1)
        for n in range(len(word) - 1, MIN_INNER - 1, -1):   # trusted pair inside a longer word
            for i in range(0, len(word) - n + 1):
                sub = word[i:i + n]
                if sub in trusted:
                    out.add(word[:i] + trusted[sub] + word[i + n:])
        return out
    first = step(a)
    if b in first:
        return True
    return any(b in step(w) for w in first)


def main():
    lines = list(parse(VARCON))

    # Words that are a correct (preferred or equal) spelling of SOME word in each variant.
    a_ok_global, b_ok_global = set(), set()
    for level, verified, rare, entries in lines:
        if rare:
            continue
        a_ok_global.update(ok(entries, "A"))
        # Only the "B" (-ise house style) column: "organize" is tagged Z (Oxford) and must
        # still be converted when the target is the default -ise British.
        b_ok_global.update(ok(entries, "B"))

    cands = []
    for level, verified, rare, entries in lines:
        if rare:
            continue
        has_z = any("Z" in c for _, c in entries)
        a_pref, b_pref = pref(entries, "A"), pref(entries, "B")
        z_pref = pref(entries, "Z") if has_z else b_pref
        a_ok, b_ok = set(ok(entries, "A")), set(ok(entries, "B"))
        z_ok = set(ok(entries, "Z")) if has_z else b_ok
        if not a_pref or not b_pref:
            continue
        a, b = a_pref[0], b_pref[0]
        z = z_pref[0] if z_pref else b
        if a == b:
            continue
        trusted = verified or level <= TRUSTED_UNVERIFIED_LEVEL
        cands.append((level, trusted, a, b, z, a_pref, b_pref, z_pref, a_ok, b_ok, z_ok))

    trusted_a2b = {}
    for (_, trusted, a, b, *_rest) in cands:
        if trusted and WORD_RE.match(a) and WORD_RE.match(b):
            trusted_a2b.setdefault(a, b)

    b2a = defaultdict(set)            # british -> {american}
    a2b = defaultdict(set)            # american -> {(british_ise, british_oxford)}
    untrusted = []
    for (level, trusted, a, b, z, a_pref, b_pref, z_pref, a_ok, b_ok, z_ok) in cands:
        if not trusted:
            if level > MAX_UNVERIFIED_LEVEL or not (WORD_RE.match(a) and WORD_RE.match(b)):
                continue
            if not explained(a, b, trusted_a2b):
                untrusted.append(f"{level}\t{a}\t{b}")
                continue
        for w in dict.fromkeys(b_pref + z_pref):          # British (either style) -> American
            if w != a and w not in a_ok:
                b2a[w].add(a)
        for w in a_pref:                                  # American -> British
            if w != b and w not in b_ok:
                oxford = w if w in z_ok else z            # what an "-ize" house style writes
                a2b[w].add((b, oxford))

    report = []
    out_b2a = {}
    for w, targets in sorted(b2a.items()):
        if not WORD_RE.match(w) or not all(WORD_RE.match(t) for t in targets):
            continue
        if w in a_ok_global:
            report.append(f"b2a\t{w}\t{'|'.join(sorted(targets))}\talso correct American")
        elif len(targets) > 1:
            report.append(f"b2a\t{w}\t{'|'.join(sorted(targets))}\tconflicting targets")
        else:
            out_b2a[w] = next(iter(targets))

    out_a2b = {}
    for w, targets in sorted(a2b.items()):
        if not WORD_RE.match(w) or not all(WORD_RE.match(b) and WORD_RE.match(o) for b, o in targets):
            continue
        if w in b_ok_global:
            report.append(f"a2b\t{w}\t{'|'.join(sorted(b for b, _ in targets))}\talso correct British")
        elif len({b for b, _ in targets}) > 1:
            report.append(f"a2b\t{w}\t{'|'.join(sorted(b for b, _ in targets))}\tconflicting targets")
        else:
            out_a2b[w] = next(iter(targets))

    # Hand-written additions / deletions (win over VarCon).
    n_extra = 0
    if os.path.exists(EXTRA):
        with open(EXTRA, encoding="utf-8") as fh:
            for raw in fh:
                line = raw.split("#")[0].rstrip()
                if not line.strip():
                    continue
                cols = line.split("\t")
                kind = cols[0]
                n_extra += 1
                if kind == "del-b2a":
                    for w in cols[1:]:
                        out_b2a.pop(w, None)
                elif kind == "del-a2b":
                    for w in cols[1:]:
                        out_a2b.pop(w, None)
                elif kind == "pair":          # american, british[, oxford]
                    a, b = cols[1], cols[2]
                    oxford = cols[3] if len(cols) > 3 else b
                    out_a2b[a] = (b, oxford)
                    out_b2a[b] = a
                elif kind == "ize":           # stem: tokenize/tokenise family
                    for stem in cols[1:]:
                        for suf in ("ize", "izes", "ized", "izing", "ization", "izations", "izer", "izers"):
                            a = stem + suf
                            b = stem + "is" + suf[2:]
                            out_a2b[a] = (b, a)
                            out_b2a[b] = a
                else:
                    raise SystemExit(f"extra_spelling.tsv: unknown kind {kind!r}")

    os.makedirs(OUT_DIR, exist_ok=True)
    header = (
        "# Generated by scripts/gen_maps.py from VarCon 2020.12.07 "
        "(c) 2000-2020 Kevin Atkinson, (c) 2016 Benjamin Titze. See LICENSE-VARCON.\n"
    )
    with open(os.path.join(OUT_DIR, "spelling_b2a.tsv"), "w", encoding="utf-8") as fh:
        fh.write(header)
        for w in sorted(out_b2a):
            fh.write(f"{w}\t{out_b2a[w]}\n")
    n_ox = 0
    with open(os.path.join(OUT_DIR, "spelling_a2b.tsv"), "w", encoding="utf-8") as fh:
        fh.write(header)
        for w in sorted(out_a2b):
            b, oxford = out_a2b[w]
            if oxford != b:
                n_ox += 1
                # "=" : the Oxford (-ize) house style keeps the American spelling
                fh.write(f"{w}\t{b}\t{'=' if oxford == w else oxford}\n")
            else:
                fh.write(f"{w}\t{b}\n")
    with open(os.path.join(ROOT, "data", "report_ambiguous.txt"), "w", encoding="utf-8") as fh:
        fh.write("# direction\tsource\ttarget(s)\treason  -- pairs NOT emitted by gen_maps.py\n")
        fh.write("\n".join(report) + "\n")
    with open(os.path.join(ROOT, "data", "report_untrusted.txt"), "w", encoding="utf-8") as fh:
        fh.write("# level\tamerican\tbritish  -- unverified VarCon pairs NOT emitted by gen_maps.py\n")
        fh.write("\n".join(untrusted) + "\n")

    print(f"varcon data lines      : {len(lines)}")
    print(f"b2a pairs              : {len(out_b2a)}")
    print(f"a2b pairs              : {len(out_a2b)}  (with a distinct Oxford form: {n_ox})")
    print(f"extra rows applied     : {n_extra}")
    print(f"dropped as ambiguous   : {len(report)}  -> data/report_ambiguous.txt")
    print(f"dropped as untrusted   : {len(untrusted)}  -> data/report_untrusted.txt")
    for name in ("spelling_b2a.tsv", "spelling_a2b.tsv"):
        p = os.path.join(OUT_DIR, name)
        print(f"{name:22s} : {os.path.getsize(p)} bytes")


if __name__ == "__main__":
    sys.exit(main())
