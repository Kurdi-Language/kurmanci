#ifndef KURMANCI_H
#define KURMANCI_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define KMR_ABI_VERSION_MAJOR UINT32_C(1)
#define KMR_ABI_VERSION_MINOR UINT32_C(0)

typedef uint32_t kmr_status;

#define KMR_OK                          UINT32_C(0)
#define KMR_ERROR_INVALID_ARGUMENT      UINT32_C(1)
#define KMR_ERROR_IO                    UINT32_C(2)
#define KMR_ERROR_INVALID_PACK          UINT32_C(3)
#define KMR_ERROR_UNSUPPORTED_PACK      UINT32_C(4)
#define KMR_ERROR_INCOMPATIBLE_LANGUAGE UINT32_C(5)
#define KMR_ERROR_CHECKSUM              UINT32_C(6)
#define KMR_ERROR_INTERNAL              UINT32_C(7)

typedef uint32_t kmr_suggestion_kind;

#define KMR_SUGGESTION_EXACT                UINT32_C(0)
#define KMR_SUGGESTION_COMPLETION           UINT32_C(1)
#define KMR_SUGGESTION_CORRECTION           UINT32_C(2)
#define KMR_SUGGESTION_DIACRITIC_CORRECTION UINT32_C(3)
#define KMR_SUGGESTION_NEXT_WORD            UINT32_C(4)

typedef uint32_t kmr_prediction_source;

#define KMR_PREDICTION_TRIGRAM        UINT32_C(0)
#define KMR_PREDICTION_BIGRAM_BACKOFF UINT32_C(1)
#define KMR_PREDICTION_BIGRAM         UINT32_C(2)
#define KMR_PREDICTION_NONE           UINT32_C(3)

typedef struct kmr_engine kmr_engine;
typedef struct kmr_suggestion_list kmr_suggestion_list;
typedef struct kmr_prediction_list kmr_prediction_list;

typedef struct {
    const char *language_tag;
    uint32_t format_version;
    size_t entry_count;
} kmr_pack_info;

typedef struct {
    const char *text;
    kmr_suggestion_kind kind;
    uint32_t edit_cost;
} kmr_suggestion_item;

typedef struct {
    const char *text;
    uint64_t count;
    uint32_t probability_millionths;
    kmr_prediction_source source;
} kmr_prediction_item;

uint32_t kmr_abi_version_major(void);
uint32_t kmr_abi_version_minor(void);

kmr_status kmr_engine_create_from_file(
    const char *path_utf8,
    kmr_engine **out_engine
);

kmr_status kmr_engine_create_from_bytes(
    const uint8_t *data,
    size_t length,
    kmr_engine **out_engine
);

void kmr_engine_destroy(kmr_engine *engine);

kmr_status kmr_engine_get_info(
    const kmr_engine *engine,
    kmr_pack_info *out_info
);

kmr_status kmr_engine_is_known_word(
    const kmr_engine *engine,
    const char *word_utf8,
    bool *out_is_known
);

kmr_status kmr_engine_correct(
    const kmr_engine *engine,
    const char *input_utf8,
    size_t limit,
    kmr_suggestion_list **out_results
);

kmr_status kmr_engine_complete(
    const kmr_engine *engine,
    const char *prefix_utf8,
    size_t limit,
    kmr_suggestion_list **out_results
);

kmr_status kmr_engine_suggest(
    const kmr_engine *engine,
    const char *input_utf8,
    size_t limit,
    kmr_suggestion_list **out_results
);

kmr_status kmr_engine_predict_next(
    const kmr_engine *engine,
    const char *const *context_words_utf8,
    size_t context_count,
    size_t limit,
    kmr_prediction_list **out_results
);

kmr_status kmr_suggestion_list_len(
    const kmr_suggestion_list *results,
    size_t *out_len
);

kmr_status kmr_suggestion_list_get(
    const kmr_suggestion_list *results,
    size_t index,
    kmr_suggestion_item *out_item
);

void kmr_suggestion_list_destroy(kmr_suggestion_list *results);

kmr_status kmr_prediction_list_len(
    const kmr_prediction_list *results,
    size_t *out_len
);

kmr_status kmr_prediction_list_get(
    const kmr_prediction_list *results,
    size_t index,
    kmr_prediction_item *out_item
);

void kmr_prediction_list_destroy(kmr_prediction_list *results);

const char *kmr_last_error_message(void);

#ifdef __cplusplus
}
#endif

#endif /* KURMANCI_H */
