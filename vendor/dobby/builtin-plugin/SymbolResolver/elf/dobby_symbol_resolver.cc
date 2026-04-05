#include "SymbolResolver/dobby_symbol_resolver.h"
#include "dobby/common.h"

#include <dlfcn.h>
#include <elf.h>
#include <fcntl.h>
#include <inttypes.h>
#include <link.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <vector>

#include "common/mmap_file_util.h"

#undef LOG_TAG
#define LOG_TAG "DobbySymbolResolver"

namespace {

constexpr int kLineMax = 2048;

struct RuntimeModule {
  void *load_address;
  char path[1024];
};

typedef struct elf_ctx {
  void *header;
  uintptr_t load_bias;
  ElfW(Shdr) *sym_sh;
  ElfW(Shdr) *dynsym_sh;
  const char *strtab;
  ElfW(Sym) *symtab;
  const char *dynstrtab;
  ElfW(Sym) *dynsymtab;
} elf_ctx_t;

static bool dobby_module_matches_name(const RuntimeModule &module, const char *image_name) {
  return image_name == NULL || strstr(module.path, image_name) != NULL;
}

static int dobby_copy_module(const RuntimeModule &module, void **out_module_load_address, char *out_path, size_t out_path_size) {
  if (module.load_address == NULL)
    return 0;

  if (out_module_load_address)
    *out_module_load_address = module.load_address;

  if (out_path && out_path_size) {
    strncpy(out_path, module.path, out_path_size - 1);
    out_path[out_path_size - 1] = '\0';
  }

  return 1;
}

static std::vector<RuntimeModule> dobby_runtime_modules() {
  std::vector<RuntimeModule> modules;

  FILE *file = fopen("/proc/self/maps", "r");
  if (file == NULL) {
    return modules;
  }

  while (!feof(file)) {
    char line_buffer[kLineMax + 1];
    if (fgets(line_buffer, kLineMax, file) == NULL) {
      break;
    }

    if (strlen(line_buffer) == kLineMax && line_buffer[kLineMax] != '\n') {
      int character = 0;
      do {
        character = getc(file);
      } while (character != EOF && character != '\n');
      if (character == EOF) {
        break;
      }
    }

    uintptr_t region_start = 0;
    uintptr_t region_end = 0;
    uintptr_t region_offset = 0;
    char permissions[5] = {'\0'};
    uint8_t dev_major = 0;
    uint8_t dev_minor = 0;
    long inode = 0;
    int path_index = 0;

    if (sscanf(
            line_buffer,
            "%" PRIxPTR "-%" PRIxPTR " %4c %" PRIxPTR " %hhx:%hhx %ld %n",
            &region_start,
            &region_end,
            permissions,
            &region_offset,
            &dev_major,
            &dev_minor,
            &inode,
            &path_index) < 7) {
      continue;
    }

    if (strcmp(permissions, "r--p") != 0 && strcmp(permissions, "r-xp") != 0) {
      continue;
    }

    auto *header = reinterpret_cast<ElfW(Ehdr) *>(region_start);
    if (memcmp(header->e_ident, ELFMAG, SELFMAG) != 0) {
      continue;
    }

    char *path = line_buffer + path_index;
    if (*path == '\0' || *path == '\n' || *path == '[') {
      continue;
    }

    RuntimeModule module = {};
    auto path_length = strlen(path);
    if (path_length > 0 && path[path_length - 1] == '\n') {
      path[path_length - 1] = '\0';
    }

    strncpy(module.path, path, sizeof(module.path) - 1);
    module.load_address = reinterpret_cast<void *>(region_start);
    modules.push_back(module);
  }

  fclose(file);

  std::sort(
      modules.begin(),
      modules.end(),
      [](const RuntimeModule &left, const RuntimeModule &right) {
        return left.load_address < right.load_address;
      });

  modules.erase(
      std::unique(
          modules.begin(),
          modules.end(),
          [](const RuntimeModule &left, const RuntimeModule &right) {
            return left.load_address == right.load_address && strcmp(left.path, right.path) == 0;
          }),
      modules.end());

  return modules;
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
    if (module.load_address == module_load_address) {
      return module;
    }
  }

  return RuntimeModule{};
}

static int elf_ctx_init(elf_ctx_t *ctx, void *header_) {
  auto *ehdr = reinterpret_cast<ElfW(Ehdr) *>(header_);
  auto ehdr_addr = reinterpret_cast<ElfW(Addr)>(ehdr);

  memset(ctx, 0, sizeof(*ctx));
  ctx->header = ehdr;

  auto *phdr = reinterpret_cast<ElfW(Phdr) *>(ehdr_addr + ehdr->e_phoff);
  for (size_t i = 0; i < ehdr->e_phnum; ++i) {
    if (phdr[i].p_type == PT_LOAD && ctx->load_bias == 0) {
      ctx->load_bias = ehdr_addr - (phdr[i].p_vaddr - phdr[i].p_offset);
    } else if (phdr[i].p_type == PT_PHDR) {
      ctx->load_bias = reinterpret_cast<ElfW(Addr)>(phdr) - phdr[i].p_vaddr;
    }
  }

  auto *shdr = reinterpret_cast<ElfW(Shdr) *>(ehdr_addr + ehdr->e_shoff);
  auto *shstr_sh = &shdr[ehdr->e_shstrndx];
  auto *shstrtab = reinterpret_cast<const char *>(ehdr_addr + shstr_sh->sh_offset);

  for (size_t i = 0; i < ehdr->e_shnum; ++i) {
    if (shdr[i].sh_type == SHT_SYMTAB) {
      ctx->sym_sh = &shdr[i];
      ctx->symtab = reinterpret_cast<ElfW(Sym) *>(ehdr_addr + shdr[i].sh_offset);
    } else if (shdr[i].sh_type == SHT_STRTAB && strcmp(shstrtab + shdr[i].sh_name, ".strtab") == 0) {
      ctx->strtab = reinterpret_cast<const char *>(ehdr_addr + shdr[i].sh_offset);
    } else if (shdr[i].sh_type == SHT_DYNSYM) {
      ctx->dynsym_sh = &shdr[i];
      ctx->dynsymtab = reinterpret_cast<ElfW(Sym) *>(ehdr_addr + shdr[i].sh_offset);
    } else if (shdr[i].sh_type == SHT_STRTAB && strcmp(shstrtab + shdr[i].sh_name, ".dynstr") == 0) {
      ctx->dynstrtab = reinterpret_cast<const char *>(ehdr_addr + shdr[i].sh_offset);
    }
  }

  return 0;
}

static void *iterate_symbol_table_impl(const char *symbol_name, ElfW(Sym) *symtab, const char *strtab, int count) {
  for (int i = 0; i < count; ++i) {
    auto *sym = symtab + i;
    auto *candidate = strtab + sym->st_name;
    if (strcmp(candidate, symbol_name) == 0) {
      return reinterpret_cast<void *>(sym->st_value);
    }
  }

  return NULL;
}

static void *elf_ctx_iterate_symbol_table(elf_ctx_t *ctx, const char *symbol_name) {
  if (ctx->symtab != NULL && ctx->strtab != NULL) {
    auto count = ctx->sym_sh->sh_size / sizeof(ElfW(Sym));
    auto *result = iterate_symbol_table_impl(symbol_name, ctx->symtab, ctx->strtab, static_cast<int>(count));
    if (result != NULL)
      return result;
  }

  if (ctx->dynsymtab != NULL && ctx->dynstrtab != NULL) {
    auto count = ctx->dynsym_sh->sh_size / sizeof(ElfW(Sym));
    return iterate_symbol_table_impl(symbol_name, ctx->dynsymtab, ctx->dynstrtab, static_cast<int>(count));
  }

  return NULL;
}

static void *dobby_resolve_symbol_in_runtime_module(const RuntimeModule &module, const char *symbol_name) {
  if (module.load_address == NULL || module.path[0] == '\0')
    return NULL;

  auto mmap_file = MmapFileManager(module.path);
  auto *file_mem = mmap_file.map();
  if (file_mem == NULL)
    return NULL;

  elf_ctx_t ctx;
  elf_ctx_init(&ctx, file_mem);
  auto *result = elf_ctx_iterate_symbol_table(&ctx, symbol_name);
  if (result == NULL)
    return NULL;

  return reinterpret_cast<void *>(
      reinterpret_cast<addr_t>(result) + reinterpret_cast<addr_t>(module.load_address) -
      (reinterpret_cast<addr_t>(file_mem) - reinterpret_cast<addr_t>(ctx.load_bias)));
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
    if (module_load_address != NULL && module.load_address != module_load_address)
      continue;
    if (!dobby_module_matches_name(module, image_name))
      continue;

    auto *result = dobby_resolve_symbol_in_runtime_module(module, symbol_name_pattern);
    if (result != NULL) {
      return result;
    }
  }

  return NULL;
}

PUBLIC void *DobbySymbolResolver(const char *image_name, const char *symbol_name_pattern) {
  if (symbol_name_pattern == NULL)
    return NULL;

  if (image_name == NULL) {
    auto *result = dlsym(RTLD_DEFAULT, symbol_name_pattern);
    if (result != NULL) {
      return result;
    }
  } else {
    auto *handle = dlopen(image_name, RTLD_NOLOAD | RTLD_NOW);
    if (handle != NULL) {
      auto *result = dlsym(handle, symbol_name_pattern);
      dlclose(handle);
      if (result != NULL) {
        return result;
      }
    }
  }

  auto modules = dobby_runtime_modules();
  for (const auto &module : modules) {
    if (!dobby_module_matches_name(module, image_name))
      continue;

    auto *result = dobby_resolve_symbol_in_runtime_module(module, symbol_name_pattern);
    if (result != NULL) {
      return result;
    }
  }

  return NULL;
}
