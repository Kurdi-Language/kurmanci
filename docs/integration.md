# Integrating the Kurmancî engine

The engine is consumed through a stable C ABI (`ffi/include/kurmanci.h`, library
`kurmanci_ffi`), wrapped by the Swift package under `swift/` and the Android SDK under
`android/`. This page covers what every integrator needs: versions, loading a pack, the five
query operations, memory ownership, threading and errors. The pack compatibility rules are in
`docs/PACK_COMPATIBILITY.md`.

## Versions

| Value | Function | Meaning |
|---|---|---|
| C ABI | `kmr_abi_version_major()`, `kmr_abi_version_minor()` | require major == 1 and minor >= the header you compiled against; minors only add |
| Engine | `kmr_engine_version()` | Rust engine crate version, static string |
| Pack schema | `kmr_supported_pack_schema_version()` | the current (native) pack schema this build writes (4); whether an arbitrary pack's schema is accepted is answered by `kmr_probe_pack_bytes(...).schema_supported` |
| Language-model schema | `kmr_language_model_schema_version()` | statistics schema carried by that pack schema (1) |
| Language | `kmr_supported_language_tag()` | `ku-Latn`; packs for any other tag are rejected |

`kmr_probe_pack_bytes(data, len, &probe)` reads a pack's declared schema, language tag,
entry count and payload length without decoding it and reports `schema_supported` and
`language_supported`, so an app can explain an unusable pack before loading.

## Load

```c
kmr_engine *engine = NULL;
kmr_status st = kmr_engine_create_from_file("lexicon.bin", &engine);   /* or _from_bytes */
if (st != KMR_OK) { log("%s: %s", kmr_status_name(st), kmr_last_error_message()); return; }
kmr_pack_info info; kmr_engine_get_info(engine, &info);   /* language_tag, format_version, entry_count */
```

A load either succeeds completely or fails with one status; there is no partial load and the
engine handle is NULL on failure. Loading never panics and never allocates more than the pack
can describe, whatever the bytes contain.

## Query

| Operation | Function | Result |
|---|---|---|
| known word | `kmr_engine_is_known_word(engine, "newroz", &known)` | `bool` |
| corrections | `kmr_engine_correct(engine, "peşeroj", limit, &list)` | suggestion list |
| completions | `kmr_engine_complete(engine, "kurd", limit, &list)` | suggestion list |
| combined | `kmr_engine_suggest(engine, "spaz", limit, &list)` | suggestion list |
| prediction | `kmr_engine_predict_next(engine, words, count, limit, &list)` | prediction list |

Inputs are NUL-terminated UTF-8; NFC and case normalization happen inside the engine, so
`Baş`, `baş` and decomposed forms query the same word. Invalid UTF-8 or a NULL pointer
returns `KMR_ERROR_INVALID_ARGUMENT`. `limit` is clamped to 50; `limit == 0` returns an empty
list. Suggestion items carry `text`, `kind` (exact, completion, correction, diacritic
correction) and `edit_cost`; prediction items carry `text`, `count`,
`probability_millionths` and `source` (trigram, bigram backoff, bigram). Results are
deterministic for a given pack and input.

## Input handling

The engine applies the repository's canonical normalization to every input before lookup,
the same rule that produced the words stored in the packs and the review identities:
Unicode control characters, U+200B (zero-width space) and U+FEFF (byte order mark) are
removed, then the text is NFC-normalized and lower-cased. The Kurmancî letters `ç ê î ş û`
are preserved as distinct letters. The SDKs pass strings through unchanged, so all surfaces
answer identically. Consequences, pinned by `engine/tests/concurrency_unicode_test.rs`,
`ffi/tests/boundary_test.rs` and the Swift and Android tests:

- precomposed and decomposed forms (for example `ş` and `s` + U+0327) and any casing query
  the same word;
- a word decorated with a byte order mark, zero-width spaces or control characters (tabs,
  NUL, C0/C1 controls) is the same word: applications do not need to strip them;
- ordinary spaces and NBSP are not removed, so ` welat` and `welat` + NBSP are different
  inputs from `welat`; tokenize on whitespace before querying;
- a token containing a hyphen or an apostrophe (`-`, U+0027, U+2019) is queried as given
  first, then, if the host wants fallbacks, as its punctuation-aware splits, with the returned
  suggestions deduplicated (the `word-punctuation-lookup` requirement of the ku-Latn keyboard
  contract). The engine answers only for the string it is given; under the project's
  word-punctuation policy such forms are held for linguist review and are not in the default
  pack, so a full-token hit is not expected until a linguist admits one;
- empty input is known-word false and yields empty completion, correction and prediction
  lists on every surface, including Kotlin; mixed ASCII and Kurmancî input is answered
  deterministically;
- malformed UTF-8 at the C boundary is rejected with `KMR_ERROR_INVALID_ARGUMENT` by every
  function before anything is read; an embedded NUL cannot be passed through C, Swift or
  Kotlin strings (it terminates or is rejected at those boundaries) even though a Rust
  `&str` may contain one and has it removed by normalization; a failing call on one thread
  never affects another thread's last error message.

## Ownership and lifetime

- Every handle the library returns (`kmr_engine`, `kmr_suggestion_list`,
  `kmr_prediction_list`) is owned by the caller and freed with the matching `*_destroy`;
  `destroy(NULL)` is a no-op; destroying twice is undefined.
- Strings inside `kmr_pack_info` and list items are borrowed: valid until the owning handle
  is destroyed, never freed by the caller. Copy them if they must outlive the handle.
- `kmr_engine_version()`, `kmr_supported_language_tag()` and `kmr_status_name()` return
  static strings valid for the process lifetime.
- `kmr_last_error_message()` is thread-local and valid until the next failing call on the
  same thread; it is never NULL (empty string when there is no error).

## Threading

A loaded `kmr_engine` is immutable. Any number of threads may call the query functions and
`kmr_engine_get_info` on the same handle concurrently, and every call returns exactly what
the same call returns single-threaded (tested with 16 threads interleaving all operations on
the Rust API and 12 threads on the C API). Creation and destruction are the caller's
responsibility to order: destroy a handle only after every thread is done with it; the
Swift wrapper ties destruction to the object's lifetime and the Kotlin SDK guards `close()`
with a read-write lock so a query never observes a freed handle. Result lists are
independent objects and may be used and destroyed on any thread. The engine keeps no
caches: 100,000 mixed queries on a resident engine leave the live heap exactly where it was
(`engine/tests/leak_test.rs`), and 500 create/query/destroy cycles plus 20,000 query and
list-destroy cycles through the C ABI return the heap to baseline (`ffi/tests/leak_test.rs`).

## Errors

Every fallible function returns a `kmr_status`; `kmr_status_name()` gives a stable symbolic
name and `kmr_last_error_message()` a human-readable detail. No Rust panic crosses the
boundary: an internal failure is reported as `KMR_ERROR_INTERNAL`.

| Status | When |
|---|---|
| `KMR_ERROR_INVALID_ARGUMENT` | NULL pointer, invalid UTF-8 input, index out of range |
| `KMR_ERROR_IO` | pack file cannot be read |
| `KMR_ERROR_INVALID_PACK` | not a pack, truncated, or a structural rule violated |
| `KMR_ERROR_UNSUPPORTED_PACK` | pack schema not supported by this build (older or newer) |
| `KMR_ERROR_INCOMPATIBLE_LANGUAGE` | pack is for another language tag |
| `KMR_ERROR_CHECKSUM` | payload checksum mismatch |
| `KMR_ERROR_INTERNAL` | contained panic or engine invariant failure |

The Swift wrapper maps these to `KurmanciError`; the Kotlin SDK to `KurmanciException`. Both
delegate all normalization, ranking and prediction to the engine; nothing is reimplemented in
the wrappers.

## Verifying a packaged build

`ffi/include/required_symbols.txt` lists every exported `kmr_*` symbol. The Rust test
`test_c_abi_header_symbol_list_and_exports_agree` keeps the header, that list and the
crate's exports identical, and `scripts/apple/verify-xcframework.sh` checks a built
XCFramework against the same list. `ffi/tests/c_smoke_test.c` is a pure C consumer that
exercises every operation against a built pack with only the header and the library.
