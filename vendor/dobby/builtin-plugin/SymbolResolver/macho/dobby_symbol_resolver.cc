#include "dobby_symbol_resolver.h"
#include "macho/dobby_symbol_resolver_priv.h"
#include "macho_file_symbol_resolver.h"

#include "dobby/common.h"

#include <mach/mach.h>
#include <mach/task.h>
#include <mach-o/dyld.h>
#include <mach-o/dyld_images.h>
#include <mach-o/loader.h>
#include <mach-o/nlist.h>

#include <algorithm>
#include <cstring>
#include <mutex>
#include <vector>

#include "macho_ctx.h"
#include "shared_cache_ctx.h"

#undef LOG_TAG
#define LOG_TAG "DobbySymbolResolver"

namespace {

struct RuntimeModule {
  void *base;
  char path[1024];
};

static bool dobby_module_matches_name(const RuntimeModule &module, const char *image_name) {
  return image_name == NULL || strstr(module.path, image_name) != NULL;
}

static int dobby_copy_module(const RuntimeModule &module, void **out_module_load_address, char *out_path, size_t out_path_size) {
  if (module.base == NULL)
    return 0;

  if (out_module_load_address)
    *out_module_load_address = module.base;

  if (out_path && out_path_size) {
    strncpy(out_path, module.path, out_path_size - 1);
    out_path[out_path_size - 1] = '\0';
  }

  return 1;
}

static std::vector<RuntimeModule> dobby_runtime_modules() {
  std::vector<RuntimeModule> modules;

  task_dyld_info_data_t task_dyld_info;
  mach_msg_type_number_t count = TASK_DYLD_INFO_COUNT;
  auto status = task_info(mach_task_self(), TASK_DYLD_INFO, reinterpret_cast<task_info_t>(&task_dyld_info), &count);
  if (status != KERN_SUCCESS) {
    return modules;
  }

  auto *infos = reinterpret_cast<dyld_all_image_infos *>(task_dyld_info.all_image_info_addr);
  if (infos == NULL) {
    return modules;
  }

  if (infos->dyldPath != NULL && infos->dyldImageLoadAddress != NULL) {
    RuntimeModule module = {};
    strncpy(module.path, infos->dyldPath, sizeof(module.path) - 1);
    module.base = reinterpret_cast<void *>(const_cast<mach_header *>(infos->dyldImageLoadAddress));
    modules.push_back(module);
  }

  if (infos->infoArray != NULL) {
    for (uint32_t i = 0; i < infos->infoArrayCount; ++i) {
      const auto &info = infos->infoArray[i];
      if (info.imageFilePath == NULL || info.imageLoadAddress == NULL) {
        continue;
      }

      RuntimeModule module = {};
      strncpy(module.path, info.imageFilePath, sizeof(module.path) - 1);
      module.base = reinterpret_cast<void *>(const_cast<mach_header *>(info.imageLoadAddress));
      modules.push_back(module);
    }
  }

  std::sort(
      modules.begin(),
      modules.end(),
      [](const RuntimeModule &left, const RuntimeModule &right) { return left.base < right.base; });

  modules.erase(
      std::unique(
          modules.begin(),
          modules.end(),
          [](const RuntimeModule &left, const RuntimeModule &right) {
            return left.base == right.base && strcmp(left.path, right.path) == 0;
          }),
      modules.end());

  return modules;
}

static shared_cache_ctx_t *dobby_shared_cache_context() {
#if defined(__arm__) || defined(__aarch64__)
  static std::once_flag once;
  static shared_cache_ctx_t context = {};
  static bool ready = false;

  std::call_once(once, []() {
    if (shared_cache_ctx_init(&context) == 0 && shared_cache_load_symbols(&context) == 0) {
      ready = true;
    }
  });

  return ready ? &context : NULL;
#else
  return NULL;
#endif
}

static void *dobby_resolve_symbol_in_runtime_module(const RuntimeModule &module, const char *symbol_name_pattern) {
  auto *header = reinterpret_cast<mach_header_t *>(module.base);
  if (header == NULL)
    return NULL;

#if defined(__arm__) || defined(__aarch64__)
  auto *shared_cache = dobby_shared_cache_context();
  if (shared_cache != NULL && shared_cache_is_contain(shared_cache, reinterpret_cast<addr_t>(header), 0)) {
    nlist_t *symtab = NULL;
    uint32_t symtab_count = 0;
    char *strtab = NULL;
    shared_cache_get_symbol_table(shared_cache, header, &symtab, &symtab_count, &strtab);
    if (symtab != NULL && strtab != NULL) {
      auto result = macho_iterate_symbol_table((char *)symbol_name_pattern, symtab, symtab_count, strtab);
      if (result != 0) {
        return reinterpret_cast<void *>(result + shared_cache->runtime_slide);
      }
    }
  }
#endif

  macho_ctx_t macho_ctx(header);
  auto result = macho_ctx.symbol_resolve(symbol_name_pattern);
  return result == 0 ? NULL : reinterpret_cast<void *>(result);
}

static RuntimeModule dobby_module_by_name(const char *image_name) {
  auto modules = dobby_runtime_modules();
  for (const auto &module : modules) {
    if (dobby_module_matches_name(module, image_name)) {
      return module;
    }
  }

  return RuntimeModule{};
}

static RuntimeModule dobby_module_by_address(void *module_load_address) {
  auto modules = dobby_runtime_modules();
  for (const auto &module : modules) {
    if (module.base == module_load_address) {
      return module;
    }
  }

  return RuntimeModule{};
}

} // namespace

PUBLIC int DobbyGetModuleByName(const char *image_name, void **out_module_load_address, char *out_path, size_t out_path_size) {
  if (image_name == NULL)
    return 0;

  return dobby_copy_module(dobby_module_by_name(image_name), out_module_load_address, out_path, out_path_size);
}

PUBLIC int DobbyGetModuleByAddress(void *module_load_address, void **out_module_load_address, char *out_path, size_t out_path_size) {
  if (module_load_address == NULL)
    return 0;

  return dobby_copy_module(dobby_module_by_address(module_load_address), out_module_load_address, out_path, out_path_size);
}

PUBLIC void *DobbyResolveSymbolInModule(const char *image_name, void *module_load_address, const char *symbol_name_pattern) {
  if (symbol_name_pattern == NULL)
    return NULL;

  auto modules = dobby_runtime_modules();
  for (const auto &module : modules) {
    if (module_load_address != NULL && module.base != module_load_address)
      continue;
    if (!dobby_module_matches_name(module, image_name))
      continue;

    auto result = dobby_resolve_symbol_in_runtime_module(module, symbol_name_pattern);
    if (result != NULL) {
      return result;
    }
  }

  return NULL;
}

PUBLIC void *DobbySymbolResolver(const char *image_name, const char *symbol_name_pattern) {
  uintptr_t result = 0;
  auto modules = dobby_runtime_modules();

  for (const auto &module : modules) {
    if (!dobby_module_matches_name(module, image_name))
      continue;

    if (image_name == NULL && strstr(module.path, "dyld") != NULL)
      continue;

    result = (uintptr_t)dobby_resolve_symbol_in_runtime_module(module, symbol_name_pattern);
    if (result != 0) {
      return (void *)result;
    }
  }

  mach_header_t *dyld_header = NULL;
  if (image_name != NULL && strcmp(image_name, "dyld") == 0) {
    task_dyld_info_data_t task_dyld_info;
    mach_msg_type_number_t count = TASK_DYLD_INFO_COUNT;
    if (task_info(mach_task_self(), TASK_DYLD_INFO, (task_info_t)&task_dyld_info, &count)) {
      return NULL;
    }

    const struct dyld_all_image_infos *infos = (struct dyld_all_image_infos *)(uintptr_t)task_dyld_info.all_image_info_addr;
    dyld_header = (mach_header_t *)infos->dyldImageLoadAddress;
    macho_ctx_t dyld_ctx(dyld_header);
    result = dyld_ctx.symbol_resolve(symbol_name_pattern);

    bool is_dyld_in_cache = ((mach_header_t *)dyld_header)->flags & MH_DYLIB_IN_CACHE;
    if (!is_dyld_in_cache && result == 0) {
      result = macho_file_symbol_resolve(
          dyld_header->cputype,
          dyld_header->cpusubtype,
          "/usr/lib/dyld",
          (char *)symbol_name_pattern);
      result += (uintptr_t)dyld_header;
    }
  }

  if (result == 0) {
    DEBUG_LOG("symbol resolver failed: %s", symbol_name_pattern);
  }

  return (void *)result;
}
