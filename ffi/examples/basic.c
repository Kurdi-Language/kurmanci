#include "kurmanci.h"
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    if (argc < 2) {
        printf("Usage: %s <path_to_lexicon.bin>\n", argv[0]);
        return 1;
    }

    const char *pack_path = argv[1];
    kmr_engine *engine = NULL;
    kmr_status status = kmr_engine_create_from_file(pack_path, &engine);
    if (status != KMR_OK) {
        fprintf(stderr, "Failed to load pack '%s': %s (code %u)\n", pack_path, kmr_last_error_message(), status);
        return 1;
    }

    kmr_pack_info info;
    if (kmr_engine_get_info(engine, &info) == KMR_OK) {
        printf("Loaded Kurmancî engine (tag: %s, format v%u, entries: %zu)\n", info.language_tag, info.format_version, info.entry_count);
    }

    bool is_known = false;
    if (kmr_engine_is_known_word(engine, "welat", &is_known) == KMR_OK) {
        printf("Is 'welat' known? %s\n", is_known ? "yes" : "no");
    }

    kmr_suggestion_list *results = NULL;
    if (kmr_engine_suggest(engine, "spaz", 5, &results) == KMR_OK && results != NULL) {
        size_t count = 0;
        if (kmr_suggestion_list_len(results, &count) == KMR_OK) {
            printf("Suggestions for 'spaz': %zu found\n", count);

            for (size_t i = 0; i < count; i++) {
                kmr_suggestion_item item;
                if (kmr_suggestion_list_get(results, i, &item) == KMR_OK) {
                    printf("  %zu. %s (edit_cost: %u, kind: %u)\n", i + 1, item.text, item.edit_cost, item.kind);
                }
            }
        }
        kmr_suggestion_list_destroy(results);
    }

    kmr_engine_destroy(engine);
    return 0;
}
