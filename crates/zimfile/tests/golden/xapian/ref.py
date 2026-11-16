# Golden results from libzim (python-libzim 3.7.0: libzim 9.3.0, Xapian 1.4.23).
# usage: python ref.py <zim> <search|suggest> <queries.txt>  →  "== q" then the top 10 paths
# Run in Docker, see README in this folder.
import sys
from libzim.reader import Archive
from libzim.search import Query, Searcher
from libzim.suggestion import SuggestionSearcher

zim, mode, queries = sys.argv[1:4]
a = Archive(zim)
for q in open(queries, encoding="utf-8").read().splitlines():
    if not q.strip():
        continue
    print("==", q)
    if mode == "search":
        s = Searcher(a).search(Query().set_query(q))
        print("#", s.getEstimatedMatches())
        paths = list(s.getResults(0, 10))
    else:
        s = SuggestionSearcher(a).suggest(q)
        print("#", s.getEstimatedMatches())
        paths = list(s.getResults(0, 10))
    for p in paths:
        print(p)
