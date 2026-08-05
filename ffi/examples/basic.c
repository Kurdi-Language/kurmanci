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
    kmr_engine_get_info(engine, &info);
    printf("Loaded Kurmancî engine (tag: %s, format v%u, entries: %zu)\n", info.language_tag, info.format_version, info.entry_count);

    bool is_known = false;
    kmr_engine_is_known_word(engine, "welat", &is_known);
    printf("Is 'welat' known? %s\n", is_known ? "yes" : "no");

    kmr_suggestion_list *results = NULL;
    kmr_engine_suggest(engine, "spaz", 5, &results);

    size_t count = 0;
    kmr_suggestion_list_len(results, &count);
    printf("Suggestions for 'spaz': %zu found\n", count);

    for (size_t i = 0; i < count; i++) {
        kmr_suggestion_item item;
        kmr_suggestion_list_get(results, i, &item);
        printf("  %zu. %s (edit_cost: %u, kind: %u)\n", i + 1, item.text, item.edit_cost, item.kind);
    }

    kmr_suggestion_list_destroy(results);
    kmr_engine_destroy(engine);
    return 0;
}
