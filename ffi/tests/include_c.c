#include "kurmanci.h"

int main(void) {
    uint32_t major = kmr_abi_version_major();
    uint32_t minor = kmr_abi_version_minor();
    return (major == KMR_ABI_VERSION_MAJOR && minor == KMR_ABI_VERSION_MINOR) ? 0 : 1;
}
