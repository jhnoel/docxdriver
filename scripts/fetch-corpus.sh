#!/usr/bin/env sh
# Fetch the coverage corpus: LibreOffice's DOCX regression fixtures
# (sw/qa/extras/ooxmlexport + ooxmlimport), thousands of small targeted
# documents. Lands in corpus/lo/ (gitignored); tests/coverage.rs picks them up.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
corpus="$root/corpus"
tmp="$corpus/.lo-src"

mkdir -p "$corpus"
if [ ! -d "$tmp/.git" ]; then
    git clone --filter=blob:none --no-checkout --depth 1 \
        https://github.com/LibreOffice/core.git "$tmp"
fi
git -C "$tmp" sparse-checkout set sw/qa/extras/ooxmlexport/data sw/qa/extras/ooxmlimport/data
git -C "$tmp" checkout HEAD

for suite in ooxmlexport ooxmlimport; do
    mkdir -p "$corpus/lo/$suite"
    find "$tmp/sw/qa/extras/$suite/data" -name '*.docx' -exec cp {} "$corpus/lo/$suite/" \;
done
rm -rf "$tmp"

echo "corpus: $(find "$corpus/lo" -name '*.docx' | wc -l | tr -d ' ') fixtures in $corpus/lo"
