#include <stdio.h>
#include <stdlib.h>
#include "kurmanci.h"

int main(void) {
    printf("Testing plain C inclusion #include <kurmanci.h>...\n");
    uint32_t major = kmr_abi_version_major();
    uint32_t minor = kmr_abi_version_minor();
    if (major != 1 || minor < 0) {
        fprintf(stderr, "❌ Incompatible C ABI: %u.%u\n", major, minor);
        return 1;
    }
    printf("✅ Plain C header inclusion verified (ABI v%u.%u)\n", major, minor);
    return 0;
}
