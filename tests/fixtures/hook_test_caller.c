#if defined(_WIN32)
#define HOOK_IMPORT __declspec(dllimport)
#define HOOK_EXPORT __declspec(dllexport)
#else
#define HOOK_IMPORT __attribute__((visibility("default")))
#define HOOK_EXPORT __attribute__((visibility("default")))
#endif

HOOK_IMPORT
int hook_test_add_one(int value);

HOOK_EXPORT
int hook_test_call_add_one(int value) {
    return hook_test_add_one(value);
}
