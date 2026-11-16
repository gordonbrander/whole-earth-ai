# Xapian reference results

`*.search.txt` and `*.suggest.txt` are libzim's results for the queries in
`*.queries` and `*.suggest`, on `wikipedia_en_climate_change_mini_2024-06.zim`
(`just testdata`) and `wikipedia_en_nollywood_maxi_2026-10.zim` (zim-mcp's test
data). Each query is `== q`, then `# <estimated matches>`, then the top 10 paths.

They come from python-libzim 3.7.0 (libzim 9.3.0, Xapian 1.4.23, the Xapian
that Kiwix's zim-tools 3.8.0 link):

```sh
cat > Dockerfile <<'X'
FROM python:3.12-slim-bookworm
RUN pip install --no-cache-dir libzim==3.7.0
X
docker build -t zimref .
docker run --rm -v $PWD:/g -v <climate.zim>:/c.zim:ro zimref \
  python /g/ref.py /c.zim search /g/climate.queries > climate.search.txt
```

## Stemmer drift (R0)

Every lowercase alphabetic word of `/usr/share/dict/words` plus the unstemmed
terms of both archives' title indexes (237,897 words), stemmed with Xapian
1.4.22's English stemmer (Debian `python3-xapian`) and in Rust:

| Stemmer | Words stemmed differently |
|---|---|
| `frostem` 1.20260821.8 (Snowball main, 2026) | 485 (0.2%): `-ologist`, `inter-` words (`international`), `added`, `emergency`, `evening` |
| `rust-stemmers` 1.2.0 (Snowball 2017) | **0** |

So zimfile stems with `rust-stemmers`, which Xapian 1.4 matches. The 7
languages it lacks (Armenian, Basque, Catalan, Indonesian, Irish, Lithuanian,
Nepali) aren't stemmed.
