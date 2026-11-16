# Check the cases `XAPIAN_EXPORT=<dir> cargo test -p zimfile --test libzim_xapian`
# wrote to <dir>/cases.tsv through real libzim (python-libzim). Exits 1 on any mismatch.
# usage: docker run --rm -v <dir>:/x -v $PWD/crates/zimfile/tests/golden/xapian:/g:ro zimref python /g/check_libzim.py /x
import os
import sys
from libzim.reader import Archive
from libzim.search import Query, Searcher
from libzim.suggestion import SuggestionSearcher

d = sys.argv[1]
bad = n = 0
for line in open(os.path.join(d, "cases.tsv"), encoding="utf-8").read().splitlines():
    zim, kind, q, limit, want = line.split("\t")
    a = Archive(os.path.join(d, zim))
    if kind == "search":
        s = Searcher(a).search(Query().set_query(q))
    else:
        s = SuggestionSearcher(a).suggest(q)
    got = list(s.getResults(0, int(limit)))
    want = want.split("|") if want else []
    n += 1
    if got != want:
        bad += 1
        print(f"MISMATCH {zim} {kind} {q!r} (limit {limit})\n  want {want}\n  got  {got}")
print(f"{n - bad}/{n} cases match libzim")
sys.exit(1 if bad else 0)
