#include "zim-mcp/src/zim/shim.h"
#include "zim-mcp/src/zim/mod.rs.h"
#include <zim/entry.h>
#include <zim/item.h>
#include <zim/search.h>
#include <zim/suggestion.h>
#include <zim/error.h>

static std::string s(rust::Str v) { return std::string(v.data(), v.size()); }

std::unique_ptr<Library> new_library() { return std::make_unique<Library>(); }

size_t open_archive(Library &lib, rust::Str path) {
  lib.archives.emplace_back(s(path));
  return lib.archives.size() - 1;
}

rust::String metadata(const Library &lib, size_t a, rust::Str name) {
  try {
    return rust::String(lib.archives.at(a).getMetadata(s(name)));
  } catch (const zim::EntryNotFound &) {
    return rust::String();
  }
}

uint32_t article_count(const Library &lib, size_t a) { return lib.archives.at(a).getArticleCount(); }
uint32_t entry_count(const Library &lib, size_t a) { return lib.archives.at(a).getEntryCount(); }
bool has_fulltext(const Library &lib, size_t a) { return lib.archives.at(a).hasFulltextIndex(); }

static EntryInfo info(const zim::Entry &e) {
  EntryInfo i;
  i.path = e.getPath();
  i.title = e.getTitle();
  i.is_redirect = e.isRedirect();
  if (i.is_redirect) {
    i.redirect_path = e.getRedirectEntry().getPath();
  } else {
    i.mimetype = e.getItem().getMimetype();
  }
  return i;
}

// Throws (→ Rust Err) when the path doesn't exist.
EntryInfo entry_by_path(const Library &lib, size_t a, rust::Str path) {
  return info(lib.archives.at(a).getEntryByPath(s(path)));
}

EntryInfo entry_at(const Library &lib, size_t a, uint32_t index) {
  return info(lib.archives.at(a).getEntryByPath(index));
}

rust::Vec<uint8_t> content(const Library &lib, size_t a, rust::Str path) {
  auto blob = lib.archives.at(a).getEntryByPath(s(path)).getItem(true).getData();
  rust::Vec<uint8_t> out;
  out.reserve(blob.size());
  for (size_t i = 0; i < blob.size(); i++) out.push_back(blob.data()[i]);
  return out;
}

rust::Vec<Hit> fulltext_search(const Library &lib, rust::Slice<const size_t> archives, rust::Str query, uint32_t limit) {
  std::vector<zim::Archive> selected;
  for (auto a : archives) selected.push_back(lib.archives.at(a));
  zim::Searcher searcher(selected);
  auto search = searcher.search(zim::Query(s(query)));
  auto results = search.getResults(0, limit);
  rust::Vec<Hit> out;
  for (auto it = results.begin(); it != results.end(); ++it) {
    Hit h;
    h.archive = archives[it.getFileIndex()];
    h.path = it.getPath();
    h.title = it.getTitle();
    h.snippet = it.getSnippet();
    h.score = it.getScore();
    out.push_back(std::move(h));
  }
  return out;
}

rust::Vec<Hit> suggest(const Library &lib, size_t a, rust::Str query, uint32_t limit) {
  zim::SuggestionSearcher searcher(lib.archives.at(a));
  auto results = searcher.suggest(s(query)).getResults(0, limit);
  rust::Vec<Hit> out;
  for (auto it = results.begin(); it != results.end(); ++it) {
    Hit h;
    h.archive = a;
    h.path = it->getPath();
    h.title = it->getTitle();
    h.snippet = it->getSnippet();
    h.score = 0;
    out.push_back(std::move(h));
  }
  return out;
}
