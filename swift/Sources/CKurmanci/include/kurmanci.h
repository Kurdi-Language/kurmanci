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
#define KMR_ABI_VERSION_MINOR 0U

/*
 * Status codes returned by all fallible kmr_* API functions.
 */
typedef uint32_t kmr_status;

#define KMR_OK                          0U
#define KMR_ERROR_INVALID_ARGUMENT      1U
#define KMR_ERROR_IO                    2U
#define KMR_ERROR_INVALID_PACK          3U
#define KMR_ERROR_UNSUPPORTED_PACK      4U
#define KMR_ERROR_INCOMPATIBLE_LANGUAGE 5U
#define KMR_ERROR_CHECKSUM              6U
#define KMR_ERROR_INTERNAL              7U

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
