#ifndef KURMANCI_H
#define KURMANCI_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Kurmancî Language Engine C ABI Version
 */
#define KMR_ABI_VERSION_MAJOR 1U
#define KMR_ABI_VERSION_MINOR 1U
/*
 * Compatibility rule for consumers: require kmr_abi_version_major() == KMR_ABI_VERSION_MAJOR
 * and kmr_abi_version_minor() >= the minor version this header was compiled against. A minor
 * version only adds symbols, struct types and status codes; it never changes existing ones.
 *
 * ABI 1.1 added: kmr_engine_version, kmr_supported_pack_schema_version,
 * kmr_language_model_schema_version, kmr_supported_language_tag, kmr_status_name,
 * kmr_probe_pack_bytes and kmr_pack_probe.
 */

/*
 * Status codes returned by all fallible kmr_* API functions.
 */
typedef uint32_t kmr_status;

#define KMR_OK                          0U
#define KMR_ERROR_INVALID_ARGUMENT      1U  /* NULL pointer, invalid UTF-8, out-of-range index */
#define KMR_ERROR_IO                    2U  /* pack file cannot be read */
#define KMR_ERROR_INVALID_PACK          3U  /* not a pack, truncated, or a structural rule violated */
#define KMR_ERROR_UNSUPPORTED_PACK      4U  /* pack schema version not supported (older or newer) */
#define KMR_ERROR_INCOMPATIBLE_LANGUAGE 5U  /* pack is for another language tag */
#define KMR_ERROR_CHECKSUM              6U  /* payload checksum mismatch */
#define KMR_ERROR_INTERNAL              7U  /* contained panic or engine invariant failure */
/*
 * Every fallible function returns exactly one of these codes and never lets a Rust panic
 * cross the boundary (a panic is reported as KMR_ERROR_INTERNAL). The mapping from load
 * conditions to codes is documented in docs/PACK_COMPATIBILITY.md.
 */

/*
 * Suggestion candidate classification kinds.
 */
typedef uint32_t kmr_suggestion_kind;

#define KMR_SUGGESTION_EXACT                0U
#define KMR_SUGGESTION_COMPLETION           1U
#define KMR_SUGGESTION_CORRECTION           2U
#define KMR_SUGGESTION_DIACRITIC_CORRECTION 3U
#define KMR_SUGGESTION_NEXT_WORD            4U

/*
 * Prediction candidate model sources.
 */
typedef uint32_t kmr_prediction_source;

#define KMR_PREDICTION_TRIGRAM        0U
#define KMR_PREDICTION_BIGRAM_BACKOFF 1U
#define KMR_PREDICTION_BIGRAM         2U
#define KMR_PREDICTION_NONE           3U

/*
 * Opaque Handles:
 * - kmr_engine: Created via kmr_engine_create_from_file / kmr_engine_create_from_bytes.
 *               Owned by caller. Must be freed with kmr_engine_destroy.
 *               Thread Safety: kmr_engine handles are immutable and thread-safe.
 *               Concurrent read-only query calls (is_known_word, correct, complete, suggest,
 *               predict_next, get_info) across multiple threads using the same engine handle
 *               are fully safe.
 * - kmr_suggestion_list: Created by kmr_engine_correct / complete / suggest.
 *                        Owned by caller. Must be freed with kmr_suggestion_list_destroy.
 * - kmr_prediction_list: Created by kmr_engine_predict_next.
 *                        Owned by caller. Must be freed with kmr_prediction_list_destroy.
 *
 * Destruction & Lifetime Rules:
 * - Calling destroy(NULL) is a safe no-op.
 * - Double-destruction of any handle is undefined caller behavior.
 * - String pointers in kmr_pack_info remain valid until the kmr_engine handle is destroyed.
 * - String pointers in kmr_suggestion_item and kmr_prediction_item remain valid until
 *   the enclosing result list handle is destroyed.
 * - Callers must copy strings if retained past handle destruction and must NEVER free string pointers.
 * - kmr_last_error_message() returns a thread-local pointer valid until the next error-producing
 *   FFI call on the same thread. It never returns a NULL or dangling pointer.
 */
typedef struct kmr_engine kmr_engine;
typedef struct kmr_suggestion_list kmr_suggestion_list;
typedef struct kmr_prediction_list kmr_prediction_list;

typedef struct {
    const char *language_tag; /* Borrowed pointer; valid until kmr_engine is destroyed */
    uint32_t format_version;
    size_t entry_count;
} kmr_pack_info;

/*
 * Size of the NUL-terminated language tag buffer in kmr_pack_probe.
 */
#define KMR_LANGUAGE_TAG_CAPACITY 32U

/*
 * Header fields of a pack read without decoding it (see kmr_probe_pack_bytes).
 */
typedef struct {
    uint32_t pack_schema_version;     /* declared by the bytes; may be unsupported */
    uint32_t entry_count;
    uint64_t payload_len;
    bool schema_supported;            /* this library loads pack_schema_version */
    bool language_supported;          /* this library serves language_tag */
    char language_tag[KMR_LANGUAGE_TAG_CAPACITY]; /* NUL-terminated UTF-8; if truncated, cut on a
                                                     character boundary, so always valid UTF-8 */
} kmr_pack_probe;

typedef struct {
    const char *text;         /* Borrowed pointer; valid until kmr_suggestion_list is destroyed */
    kmr_suggestion_kind kind;
    uint32_t edit_cost;
} kmr_suggestion_item;

typedef struct {
    const char *text;         /* Borrowed pointer; valid until kmr_prediction_list is destroyed */
    uint64_t count;
    uint32_t probability_millionths;
    kmr_prediction_source source;
} kmr_prediction_item;

uint32_t kmr_abi_version_major(void);
uint32_t kmr_abi_version_minor(void);

/*
 * Build identity and compatibility of this library. The returned strings are static,
 * NUL-terminated, never NULL, valid for the lifetime of the process and must not be freed.
 * See docs/PACK_COMPATIBILITY.md for the rules these values feed.
 *
 * kmr_supported_pack_schema_version() is the current (native) pack schema version, the one
 * this library's packs are written in. A future library may load more than one schema, so
 * whether an arbitrary pack's schema is accepted must be checked at runtime with
 * kmr_probe_pack_bytes(...).schema_supported, not by comparing against this value.
 */
const char *kmr_engine_version(void);
uint32_t kmr_supported_pack_schema_version(void);
uint32_t kmr_language_model_schema_version(void);
const char *kmr_supported_language_tag(void);

/*
 * Stable symbolic name of a status code (e.g. "KMR_ERROR_UNSUPPORTED_PACK") for logs.
 * Static string, never NULL; unknown codes yield "KMR_STATUS_UNKNOWN".
 */
const char *kmr_status_name(kmr_status status);

/*
 * Reads the header of a pack in memory without decoding it, so that a consumer can report
 * why a pack is unusable before attempting a load. On success fills *out_probe; an
 * unsupported schema version or language is reported through the schema_supported /
 * language_supported flags, not as an error. Returns KMR_ERROR_INVALID_PACK when the bytes
 * are not a pack at all or are shorter than the header. Rejects data == NULL.
 */
kmr_status kmr_probe_pack_bytes(
    const uint8_t *data,
    size_t length,
    kmr_pack_probe *out_probe
);

/*
 * Creates an engine by loading a binary pack file from path_utf8.
 * On success, populates *out_engine with a new handle owned by the caller.
 * On failure, sets *out_engine to NULL and returns an error status code.
 */
kmr_status kmr_engine_create_from_file(
    const char *path_utf8,
    kmr_engine **out_engine
);

/*
 * Creates an engine by loading a binary pack from memory buffer `data` of size `length`.
 * Rejects data == NULL even if length == 0.
 * On success, populates *out_engine with a new handle owned by the caller.
 * On failure, sets *out_engine to NULL and returns an error status code.
 */
kmr_status kmr_engine_create_from_bytes(
    const uint8_t *data,
    size_t length,
    kmr_engine **out_engine
);

/*
 * Destroys a kmr_engine handle. Passing NULL is a safe no-op.
 * Passing an invalid or already-destroyed handle is undefined caller behavior.
 */
void kmr_engine_destroy(kmr_engine *engine);

/*
 * Fills out_info with pack metadata.
 * The out_info->language_tag string remains valid until engine is destroyed.
 */
kmr_status kmr_engine_get_info(
    const kmr_engine *engine,
    kmr_pack_info *out_info
);

/*
 * Checks if word_utf8 exists in the loaded lexicon.
 * Populates *out_is_known with the result.
 */
kmr_status kmr_engine_is_known_word(
    const kmr_engine *engine,
    const char *word_utf8,
    bool *out_is_known
);

/*
 * Generates spelling correction candidates for input_utf8.
 * On success, populates *out_results with a suggestion list owned by caller.
 * Result limits are clamped to a maximum of 50. Limit 0 returns an empty list.
 */
kmr_status kmr_engine_correct(
    const kmr_engine *engine,
    const char *input_utf8,
    size_t limit,
    kmr_suggestion_list **out_results
);

/*
 * Generates prefix completion candidates for prefix_utf8.
 * On success, populates *out_results with a suggestion list owned by caller.
 */
kmr_status kmr_engine_complete(
    const kmr_engine *engine,
    const char *prefix_utf8,
    size_t limit,
    kmr_suggestion_list **out_results
);

/*
 * Generates combined suggestions (exact, completion, correction) for input_utf8.
 * On success, populates *out_results with a suggestion list owned by caller.
 */
kmr_status kmr_engine_suggest(
    const kmr_engine *engine,
    const char *input_utf8,
    size_t limit,
    kmr_suggestion_list **out_results
);

/*
 * Predicts next words following given UTF-8 context words array context_words_utf8.
 * context_words_utf8 == NULL is valid ONLY when context_count == 0.
 * On success, populates *out_results with a prediction list owned by caller.
 */
kmr_status kmr_engine_predict_next(
    const kmr_engine *engine,
    const char *const *context_words_utf8,
    size_t context_count,
    size_t limit,
    kmr_prediction_list **out_results
);

/*
 * Returns the number of items in a kmr_suggestion_list.
 */
kmr_status kmr_suggestion_list_len(
    const kmr_suggestion_list *results,
    size_t *out_len
);

/*
 * Retrieves the item at `index` in a kmr_suggestion_list into *out_item.
 * out_item->text is a borrowed string valid until results is destroyed.
 */
kmr_status kmr_suggestion_list_get(
    const kmr_suggestion_list *results,
    size_t index,
    kmr_suggestion_item *out_item
);

/*
 * Destroys a kmr_suggestion_list and frees all item strings. Passing NULL is a safe no-op.
 */
void kmr_suggestion_list_destroy(kmr_suggestion_list *results);

/*
 * Returns the number of items in a kmr_prediction_list.
 */
kmr_status kmr_prediction_list_len(
    const kmr_prediction_list *results,
    size_t *out_len
);

/*
 * Retrieves the item at `index` in a kmr_prediction_list into *out_item.
 * out_item->text is a borrowed string valid until results is destroyed.
 */
kmr_status kmr_prediction_list_get(
    const kmr_prediction_list *results,
    size_t index,
    kmr_prediction_item *out_item
);

/*
 * Destroys a kmr_prediction_list and frees all item strings. Passing NULL is a safe no-op.
 */
void kmr_prediction_list_destroy(kmr_prediction_list *results);

/*
 * Returns the last thread-local error message as a NUL-terminated UTF-8 string pointer.
 * Returns an empty string "" if no error has occurred. Never returns NULL or a dangling pointer.
 * The pointer remains valid until the next error-producing FFI call on the same thread.
 */
const char *kmr_last_error_message(void);

#ifdef __cplusplus
}
#endif

#endif /* KURMANCI_H */
