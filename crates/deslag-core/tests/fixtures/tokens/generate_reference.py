# Regenerates the offline reference token counts for the estimator calibration.
#
# Pinned tool: tiktoken==0.14.0 (encodings o200k_base and cl100k_base).
# These are other-family tokenizers. Their counts are references for
# calibration, not Claude counts.
#
# Usage, from the repository root, in a throwaway virtualenv outside the
# repository:
#   pip install tiktoken==0.14.0
#   python -I crates/deslag-core/tests/fixtures/tokens/generate_reference.py crates/deslag-core/tests/fixtures/tokens
#
# Writes the minified JSON fixtures (*.min.json) and reference.tsv.
#
# The minified files come from json.dumps, which normalizes some lexemes:
# number exponents, string escapes, non-ASCII escapes and duplicate keys.
# The pretty fixtures must avoid them. The Rust test
# minified_fixtures_match_the_core_minifier guards this: it fails when
# json.dumps output differs from deslag_core::minify_json.
import json
import sys
from pathlib import Path

import tiktoken

ENCODINGS = ["o200k_base", "cl100k_base"]
PRETTY_JSON = ["pretty.json", "records.json", "nested.json", "nested_tab.json"]
TEXTS = ["log.txt", "source_code.txt", "prose.txt", "cjk.txt"]


def main(root: Path) -> None:
    files = []
    for name in PRETTY_JSON:
        value = json.loads((root / name).read_text(encoding="utf-8"))
        minified = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
        min_name = name.replace(".json", ".min.json")
        (root / min_name).write_text(minified, encoding="utf-8")
        files += [name, min_name]
    files += TEXTS

    encoders = [tiktoken.get_encoding(e) for e in ENCODINGS]
    lines = ["file\t" + "\t".join(ENCODINGS)]
    for name in files:
        text = (root / name).read_text(encoding="utf-8")
        counts = [str(len(enc.encode(text))) for enc in encoders]
        lines.append(name + "\t" + "\t".join(counts))
    (root / "reference.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
