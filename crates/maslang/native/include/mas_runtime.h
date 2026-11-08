#ifndef MAS_RUNTIME_H
#define MAS_RUNTIME_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif

/* UTF-8 字符串字段布局；返回 string 的 extern 函数仍使用指针返回值和 out_len 尾参。 */
typedef struct MasString { const uint8_t *ptr; uint64_t len; } MasString;

/* 分配八字节对齐的零初始化托管对象，返回值归 GC 所有，不得 free/realloc。
 * offsets 列出载荷中每个托管指针的字节偏移（字符串列 ptr）；表必须具有静态生命周期。
 * count 为零时 offsets 可为 NULL；不在分配函数内触发回收。 */
void *mas_record_alloc(uint64_t size, const uint64_t *offsets, uint64_t count);

/* 复制 UTF-8 字节到 GC 字符串，返回稳定的 ptr+len；调用方不得释放指针。 */
MasString mas_runtime_string_clone(const uint8_t *ptr, uint64_t len);

/* 使用运行时统一错误处理终止进程；message 必须是以零结尾的字符串。 */
void mas_runtime_abort(const char *message);
#ifdef __cplusplus
}
#endif
#endif
