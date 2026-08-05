#include "kurmanci.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <path_to_lexicon.bin>\n", argv[0]);
        return 1;
    }

    printf("Testing Kurmancî C ABI v%u.%u...\n", kmr_abi_version_major(), kmr_abi_version_minor());

    /* 1. Test invalid file path error */
    kmr_engine *bad_engine = NULL;
    kmr_status status = kmr_engine_create_from_file("nonexistent_pack_path_123.bin", &bad_engine);
    if (status != KMR_ERROR_IO) {
        fprintf(stderr, "Expected KMR_ERROR_IO, got %u: %s\n", status, kmr_last_error_message());
        return 1;
    }
    if (bad_engine != NULL) {
        fprintf(stderr, "bad_engine out pointer not reset to NULL on error!\n");
        return 1;
    }
    printf("✅ Nonexistent file correctly rejected with KMR_ERROR_IO\n");

    /* 2. Load engine from file */
    const char *pack_path = argv[1];
    kmr_engine *engine = NULL;
    status = kmr_engine_create_from_file(pack_path, &engine);
    if (status != KMR_OK || engine == NULL) {
        fprintf(stderr, "Failed to load engine from '%s': %u (%s)\n", pack_path, status, kmr_last_error_message());
        return 1;
    }
    printf("✅ Engine successfully loaded from file: %s\n", pack_path);

    /* 3. Check Pack Info */
    kmr_pack_info info;
    status = kmr_engine_get_info(engine, &info);
    if (status != KMR_OK) {
        fprintf(stderr, "kmr_engine_get_info failed: %u\n", status);
        return 1;
    }
    printf("  Pack tag: %s, format v%u, entries: %zu\n", info.language_tag, info.format_version, info.entry_count);

    /* 4. Test is_known_word */
    bool is_known = false;
    status = kmr_engine_is_known_word(engine, "welat", &is_known);
    if (status != KMR_OK || !is_known) {
        fprintf(stderr, "Expected 'welat' to be known, got status %u, is_known: %d\n", status, is_known);
        return 1;
    }
    printf("✅ 'welat' is known word\n");

    status = kmr_engine_is_known_word(engine, "nonexistent123", &is_known);
    if (status != KMR_OK || is_known) {
        fprintf(stderr, "Expected 'nonexistent123' to be unknown, got status %u, is_known: %d\n", status, is_known);
        return 1;
    }
    printf("✅ 'nonexistent123' is unknown word\n");

    /* 5. Test corrections */
    kmr_suggestion_list *corrections = NULL;
    status = kmr_engine_correct(engine, "spaz", 5, &corrections);
    if (status != KMR_OK || corrections == NULL) {
        fprintf(stderr, "kmr_engine_correct failed: %u\n", status);
        return 1;
    }
    size_t corr_len = 0;
    kmr_suggestion_list_len(corrections, &corr_len);
    printf("✅ Corrections for 'spaz': %zu results\n", corr_len);
    if (corr_len > 0) {
        kmr_suggestion_item item;
        kmr_suggestion_list_get(corrections, 0, &item);
        printf("  Top correction: %s (cost: %u, kind: %u)\n", item.text, item.edit_cost, item.kind);
    }
    kmr_suggestion_list_destroy(corrections);

    /* 6. Test completions */
    kmr_suggestion_list *completions = NULL;
    status = kmr_engine_complete(engine, "roj", 5, &completions);
    if (status != KMR_OK || completions == NULL) {
        fprintf(stderr, "kmr_engine_complete failed: %u\n", status);
        return 1;
    }
    size_t comp_len = 0;
    kmr_suggestion_list_len(completions, &comp_len);
    printf("✅ Completions for 'roj': %zu results\n", comp_len);
    kmr_suggestion_list_destroy(completions);

    /* 7. Test predictions */
    const char *context[2] = {"ez", "diçim"};
    kmr_prediction_list *predictions = NULL;
    status = kmr_engine_predict_next(engine, context, 2, 5, &predictions);
    if (status != KMR_OK || predictions == NULL) {
        fprintf(stderr, "kmr_engine_predict_next failed: %u\n", status);
        return 1;
    }
    size_t pred_len = 0;
    kmr_prediction_list_len(predictions, &pred_len);
    printf("✅ Predictions following ['ez', 'diçim']: %zu results\n", pred_len);
    kmr_prediction_list_destroy(predictions);

    /* 8. Test kmr_engine_create_from_bytes */
    FILE *f = fopen(pack_path, "rb");
    if (!f) {
        fprintf(stderr, "Failed to open pack file for bytes test\n");
        return 1;
    }
    fseek(f, 0, SEEK_END);
    long fsize = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *bytes = (uint8_t *)malloc(fsize);
    if (fread(bytes, 1, fsize, f) != (size_t)fsize) {
        fprintf(stderr, "Failed to read bytes from file\n");
        fclose(f);
        free(bytes);
        return 1;
    }
    fclose(f);

    kmr_engine *bytes_engine = NULL;
    status = kmr_engine_create_from_bytes(bytes, fsize, &bytes_engine);
    free(bytes);
    if (status != KMR_OK || bytes_engine == NULL) {
        fprintf(stderr, "kmr_engine_create_from_bytes failed: %u\n", status);
        return 1;
    }
    printf("✅ Engine created successfully from raw bytes (%ld bytes)\n", fsize);

    kmr_engine_destroy(bytes_engine);
    kmr_engine_destroy(engine);

    /* Destroying NULL must be safe no-op */
    kmr_engine_destroy(NULL);
    kmr_suggestion_list_destroy(NULL);
    kmr_prediction_list_destroy(NULL);

    printf("⚡ All C ABI Smoke Tests PASSED successfully!\n");
    return 0;
}
