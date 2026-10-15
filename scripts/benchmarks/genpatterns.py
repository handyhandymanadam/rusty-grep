#!/usr/bin/env python3
"""Generate ~250 test patterns from the kernel source vocabulary (deterministic)."""
import collections, json, random, re
random.seed(20261004)
text = open("../big.txt", "rb").read().decode("utf-8", "replace")
words = collections.Counter(re.findall(r"\b[A-Za-z_][A-Za-z_0-9]{2,23}\b", text))
by_freq = [w for w, _ in words.most_common()]
common = [w for w in by_freq[:300] if len(w) >= 4]
mid = [w for w in by_freq[300:6000]]
rare = [w for w, c in words.items() if 3 <= c <= 30]
short = [w for w in by_freq[:3000] if 3 <= len(w) <= 4]
lines = [l for l in text.splitlines() if 20 < len(l) < 120]
P = []
def add(cat, pat, rg=(), raw=None):
    P.append(dict(id=len(P), cat=cat, pat=pat, rg=list(rg), raw=list(raw if raw is not None else rg)))
for w in random.sample(common, 20): add("literal-common", w)
for w in random.sample(rare, 25): add("literal-rare", w)
for _ in range(10): add("literal-absent", "zq" + "".join(random.choice("xkjvw") for _ in range(8)))
for w in random.sample(short, 20): add("literal-short", w)
for w in random.sample(mid, 15): add("word-boundary-w", w, ["-w"])
for w in random.sample(mid, 20): add("ignore-case", w, ["-i"])
for k in (2, 3, 4, 6):
    for _ in range(8): add(f"alternation-{k}", "|".join(random.sample(mid, k)))
for p in ["#include <linux", "static", "\\t", "\\s*if", "\\s*return", "#define", "\\*", "//", "}", "struct", "\\s*/\\*", "EXPORT_SYMBOL", "#ifdef", "#endif", "int "]:
    add("anchor-start", "^" + p)
for p in [";", "\\{", "\\)", ",", "\\\\", "\\*/", "\\}", "0", "\\);", "\\) \\{"]:
    add("anchor-end", p + "$")
add("anchor-both", "^$"); add("anchor-both", "^\\s*$"); add("anchor-both", "^\\}$")
for p in ["[A-Z]+_[A-Z]+", "0x[0-9a-f]+", "[0-9]+", "u(8|16|32|64)\\b", "\\w+_lock\\(", "[a-z]+_t\\b", "__[a-z]+__", "\\bNULL\\b", "[<>]=?", "\\d{4}-\\d{2}",
          "[A-Z][a-z]+[A-Z]", "(?:un)?likely\\(", "[a-z_]+\\(void\\)", "\\bint\\s+\\w+\\s*=", "0[xX][0-9A-Fa-f]{8}\\b", "\\bkfree\\b|\\bkmalloc\\b",
          "foo|bar|baz", "colou?r", "a{2,3}b", "(?i)todo", "\\w{12,}", "\\b[A-Z]{4,}\\b", "[^a-z\\s]{6}", ".{80,}", "\\S+@\\S+", "\\$\\(\\w+\\)"]:
    add("class-repetition", p)
for _ in range(25):
    l = random.choice(lines); toks = re.findall(r"[A-Za-z_][A-Za-z_0-9]{3,}", l)
    if len(toks) >= 2:
        a, b = toks[0], toks[-1]
        if a != b: add("inner-required", f"{re.escape(a)}.*{re.escape(b)}")
for p in ["é", "ü", "µ", "—", "ß", "ø", "→", "ñ"]: add("unicode", p)
for p in ["foo\\(", "\\.c\\b", "\\$\\(", "a\\+b", "\\[0\\]", "x\\|y", "->", "\\\\n", "\"%s\"", "\\*\\*"]: add("escaped-meta", p)
for p in ["x*", "a?", "\\b", "(?:)"]: add("matches-everything", p)
for p in ["\\s+$", "^\\s+", "\\t+ ", "  +$", "\\r"]: add("whitespace", p)
for c in ("common", "mid"):
    pool = common if c == "common" else mid
    for w in random.sample(pool, 8): add("case-literal-mixed", w[0].upper() + w[1:])
json.dump(P, open("patterns.json", "w"), indent=0)
print(len(P), "patterns;", collections.Counter(p["cat"] for p in P).most_common(6), "...")
