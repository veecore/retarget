#if defined(_WIN32)
#define HOOK_EXPORT __declspec(dllexport)
#else
#define HOOK_EXPORT __attribute__((visibility("default")))
#endif

HOOK_EXPORT
int hook_test_add_one(int value) {
    return value + 1;
}
