// DuckDB's prebuilt static library keeps its core extension in a separate archive that nothing
// registers on its own; this constructor does, before main, so SQL such as `sum` resolves.
#include <stdint.h>

int32_t duckdb_extension_core_functions_describe(void *descriptor);
int32_t duckdb_register_static_extension(int32_t (*describe)(void *));

__attribute__((constructor)) static void register_core_functions(void) {
    duckdb_register_static_extension(duckdb_extension_core_functions_describe);
}
