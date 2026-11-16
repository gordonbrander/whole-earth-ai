// Thin C++ shim over libzim, exposed to Rust through the cxx crate.
// Covers only what zim-mcp needs: open, metadata, entry lookup, content,
// full-text search and title suggestions.
#pragma once
#include <memory>
#include <vector>
#include <zim/archive.h>
#include "rust/cxx.h"

struct EntryInfo;
struct Hit;

class Library {
public:
  std::vector<zim::Archive> archives;
};

std::unique_ptr<Library> new_library();
size_t open_archive(Library &lib, rust::Str path);
rust::String metadata(const Library &lib, size_t archive, rust::Str name);
uint32_t article_count(const Library &lib, size_t archive);
uint32_t entry_count(const Library &lib, size_t archive);
bool has_fulltext(const Library &lib, size_t archive);
EntryInfo entry_by_path(const Library &lib, size_t archive, rust::Str path);
EntryInfo entry_at(const Library &lib, size_t archive, uint32_t index);
rust::Vec<uint8_t> content(const Library &lib, size_t archive, rust::Str path);
rust::Vec<Hit> fulltext_search(const Library &lib, rust::Slice<const size_t> archives, rust::Str query, uint32_t limit);
rust::Vec<Hit> suggest(const Library &lib, size_t archive, rust::Str query, uint32_t limit);
