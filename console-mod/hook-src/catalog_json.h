/* Bounded, dependency-free JSON DOM for one catalogue snapshot. Strings are
 * UTF-8, decoded once by the IO worker; indices survive arena reallocations. */
#ifndef CHRONOS_CATALOG_JSON_H
#define CHRONOS_CATALOG_JSON_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <limits.h>
#include <math.h>

#define CATALOG_JSON_MAX_BYTES (256u * 1024u)
#define CATALOG_JSON_MAX_NODES 16384u
#define CATALOG_JSON_MAX_DEPTH 16u
#define CATALOG_JSON_FILE_LIMIT (32u * 1024u)
enum catalog_json_type { CJ_NULL, CJ_BOOL, CJ_INT, CJ_FLOAT, CJ_STRING, CJ_ARRAY, CJ_OBJECT };
struct catalog_json_node {
    uint32_t child, next;
    union { uint32_t string; int32_t integer; float number; } value;
    uint16_t count;
    uint8_t type, reserved;
};
struct catalog_json {
    struct catalog_json_node *nodes;
    char *strings;
    uint32_t node_count, node_capacity, string_size, string_capacity;
};
struct catalog_json_parser { struct catalog_json *dom; const char *cursor, *end; };

static void catalog_json_free(struct catalog_json *dom)
{ free(dom->nodes); free(dom->strings); memset(dom, 0, sizeof(*dom)); }

static uint32_t catalog_json_new(struct catalog_json *dom, unsigned type)
{
    if (dom->node_count + 1 >= CATALOG_JSON_MAX_NODES) { errno = E2BIG; return 0; }
    if (dom->node_count + 1 >= dom->node_capacity) {
        uint32_t cap = dom->node_capacity ? dom->node_capacity * 2 : 128;
        if (cap > CATALOG_JSON_MAX_NODES) cap = CATALOG_JSON_MAX_NODES;
        void *p = realloc(dom->nodes, cap * sizeof(*dom->nodes));
        if (!p) return 0;
        dom->nodes = p; dom->node_capacity = cap;
    }
    uint32_t id = ++dom->node_count;
    memset(&dom->nodes[id], 0, sizeof(dom->nodes[id])); dom->nodes[id].type = (uint8_t)type;
    return id;
}

static int catalog_json_byte(struct catalog_json *dom, unsigned char byte)
{
    if (dom->string_size == CATALOG_JSON_MAX_BYTES) { errno = E2BIG; return -1; }
    if (dom->string_size == dom->string_capacity) {
        uint32_t cap = dom->string_capacity ? dom->string_capacity * 2 : 4096;
        if (cap > CATALOG_JSON_MAX_BYTES) cap = CATALOG_JSON_MAX_BYTES;
        void *p = realloc(dom->strings, cap);
        if (!p) return -1;
        dom->strings = p; dom->string_capacity = cap;
    }
    dom->strings[dom->string_size++] = (char)byte;
    return 0;
}

static uint32_t catalog_json_string_copy(struct catalog_json *dom, const char *string)
{
    uint32_t node = catalog_json_new(dom, CJ_STRING), start = dom->string_size;
    if (!node) return 0;
    do { if (catalog_json_byte(dom, (unsigned char)*string)) return 0; } while (*string++);
    dom->nodes[node].value.string = start;
    return node;
}

static const char *catalog_json_string(const struct catalog_json *dom, uint32_t node)
{ return node && node <= dom->node_count && dom->nodes[node].type == CJ_STRING
         ? dom->strings + dom->nodes[node].value.string : NULL; }

static uint32_t catalog_json_member(const struct catalog_json *dom, uint32_t node, const char *name)
{
    if (!node || dom->nodes[node].type != CJ_OBJECT) return 0;
    for (uint32_t key = dom->nodes[node].child; key;) {
        uint32_t value = dom->nodes[key].next;
        if (!strcmp(catalog_json_string(dom, key), name)) return value;
        key = dom->nodes[value].next;
    }
    return 0;
}

static void catalog_json_space(struct catalog_json_parser *p)
{ while (p->cursor < p->end && strchr(" \t\r\n", *p->cursor) && *p->cursor) ++p->cursor; }

static int catalog_json_hex(struct catalog_json_parser *p, uint32_t *out)
{
    uint32_t value = 0;
    if (p->end - p->cursor < 4) return -1;
    for (unsigned i = 0; i < 4; ++i) {
        unsigned char c = (unsigned char)*p->cursor++;
        unsigned v = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 :
                     c >= 'A' && c <= 'F' ? c - 'A' + 10 : 16;
        if (v > 15) return -1;
        value = value * 16 + v;
    }
    *out = value; return 0;
}

static int catalog_json_codepoint(struct catalog_json *dom, uint32_t c)
{
    if (!c || c > 0x10ffff || (c >= 0xd800 && c <= 0xdfff)) return -1;
    if (c < 0x80) return catalog_json_byte(dom, (unsigned char)c);
    if (c < 0x800) {
        if (catalog_json_byte(dom, (unsigned char)(0xc0 | (c >> 6)))) return -1;
    } else {
        if (c >= 0x10000 && catalog_json_byte(dom, (unsigned char)(0xf0 | (c >> 18)))) return -1;
        if (catalog_json_byte(dom, (unsigned char)((c < 0x10000 ? 0xe0 : 0x80) | ((c >> 12) & 0x3f)))) return -1;
        if (catalog_json_byte(dom, (unsigned char)(0x80 | ((c >> 6) & 0x3f)))) return -1;
    }
    return catalog_json_byte(dom, (unsigned char)(0x80 | (c & 0x3f)));
}

static uint32_t catalog_json_parse_string(struct catalog_json_parser *p)
{
    struct catalog_json *dom = p->dom;
    uint32_t node = catalog_json_new(dom, CJ_STRING), start = dom->string_size;
    if (!node || p->cursor == p->end || *p->cursor++ != '"') return 0;
    while (p->cursor < p->end) {
        unsigned char c = (unsigned char)*p->cursor++;
        if (c == '"') {
            if (catalog_json_byte(dom, 0)) return 0;
            dom->nodes[node].value.string = start; return node;
        }
        if (c < 0x20) return 0;
        if (c == '\\') {
            if (p->cursor == p->end) return 0;
            c = (unsigned char)*p->cursor++;
            const char *escapes = "\"\\/bfnrt", *found = strchr(escapes, c);
            if (found && c) {
                static const unsigned char values[] = {'"','\\','/','\b','\f','\n','\r','\t'};
                if (catalog_json_byte(dom, values[found - escapes])) return 0;
                continue;
            }
            if (c != 'u') return 0;
            uint32_t code;
            if (catalog_json_hex(p, &code)) return 0;
            if (code >= 0xd800 && code <= 0xdbff) {
                if (p->end - p->cursor < 6 || memcmp(p->cursor, "\\u", 2)) return 0;
                p->cursor += 2; uint32_t low;
                if (catalog_json_hex(p, &low) || low < 0xdc00 || low > 0xdfff) return 0;
                code = 0x10000 + (code - 0xd800) * 1024 + low - 0xdc00;
            }
            if (catalog_json_codepoint(dom, code)) return 0;
        } else if (c < 0x80) {
            if (catalog_json_byte(dom, c)) return 0;
        } else {
            unsigned remaining = c >= 0xc2 && c <= 0xdf ? 1 : c >= 0xe0 && c <= 0xef ? 2 :
                                 c >= 0xf0 && c <= 0xf4 ? 3 : 0;
            if (!remaining || p->end - p->cursor < (int)remaining) return 0;
            unsigned char first = (unsigned char)*p->cursor;
            if ((c == 0xe0 && first < 0xa0) || (c == 0xed && first >= 0xa0) ||
                (c == 0xf0 && first < 0x90) || (c == 0xf4 && first >= 0x90)) return 0;
            if (catalog_json_byte(dom, c)) return 0;
            while (remaining--) {
                c = (unsigned char)*p->cursor++;
                if ((c & 0xc0) != 0x80 || catalog_json_byte(dom, c)) return 0;
            }
        }
    }
    return 0;
}

static uint32_t catalog_json_parse_value(struct catalog_json_parser *p, unsigned depth)
{
    if (depth > CATALOG_JSON_MAX_DEPTH) { errno = E2BIG; return 0; }
    catalog_json_space(p);
    if (p->cursor == p->end) return 0;
    if (*p->cursor == '"') return catalog_json_parse_string(p);
    if (*p->cursor == '{' || *p->cursor == '[') {
        int object = *p->cursor++ == '{'; char end = object ? '}' : ']';
        uint32_t node = catalog_json_new(p->dom, object ? CJ_OBJECT : CJ_ARRAY), previous = 0;
        if (!node) return 0;
        catalog_json_space(p);
        if (p->cursor < p->end && *p->cursor == end) { ++p->cursor; return node; }
        for (;;) {
            uint32_t key = 0, child;
            if (object) {
                key = catalog_json_parse_string(p);
                if (!key || !strcmp(catalog_json_string(p->dom, key), "__chronos_array") ||
                    catalog_json_member(p->dom, node, catalog_json_string(p->dom, key))) return 0;
                catalog_json_space(p);
                if (p->cursor == p->end || *p->cursor++ != ':') return 0;
            }
            child = catalog_json_parse_value(p, depth + 1);
            if (!child) return 0;
            if (key) p->dom->nodes[key].next = child;
            uint32_t first = key ? key : child;
            if (previous) p->dom->nodes[previous].next = first;
            else p->dom->nodes[node].child = first;
            previous = child;
            ++p->dom->nodes[node].count;
            catalog_json_space(p);
            if (p->cursor == p->end) return 0;
            if (*p->cursor == end) { ++p->cursor; return node; }
            if (*p->cursor++ != ',') return 0;
            catalog_json_space(p);
        }
    }
    const char *literal[] = {"null", "false", "true"};
    for (unsigned i = 0; i < 3; ++i) {
        size_t n = strlen(literal[i]);
        if ((size_t)(p->end - p->cursor) >= n && !memcmp(p->cursor, literal[i], n)) {
            p->cursor += n; uint32_t node = catalog_json_new(p->dom, i ? CJ_BOOL : CJ_NULL);
            if (node) p->dom->nodes[node].value.integer = i == 2;
            return node;
        }
    }
    const char *start = p->cursor;
    if (*p->cursor == '-') ++p->cursor;
    if (p->cursor == p->end || *p->cursor < '0' || *p->cursor > '9') return 0;
    if (*p->cursor == '0') ++p->cursor;
    else while (p->cursor < p->end && *p->cursor >= '0' && *p->cursor <= '9') ++p->cursor;
    int floating = 0;
    if (p->cursor < p->end && *p->cursor == '.') {
        floating = 1; ++p->cursor; const char *digits = p->cursor;
        while (p->cursor < p->end && *p->cursor >= '0' && *p->cursor <= '9') ++p->cursor;
        if (p->cursor == digits) return 0;
    }
    if (p->cursor < p->end && (*p->cursor == 'e' || *p->cursor == 'E')) {
        floating = 1; ++p->cursor;
        if (p->cursor < p->end && (*p->cursor == '+' || *p->cursor == '-')) ++p->cursor;
        const char *digits = p->cursor;
        while (p->cursor < p->end && *p->cursor >= '0' && *p->cursor <= '9') ++p->cursor;
        if (p->cursor == digits) return 0;
    }
    char number[64]; size_t length = (size_t)(p->cursor - start);
    if (!length || length >= sizeof(number)) return 0;
    memcpy(number, start, length); number[length] = 0;
    uint32_t node = catalog_json_new(p->dom, floating ? CJ_FLOAT : CJ_INT);
    if (!node) return 0;
    errno = 0;
    if (floating) {
        float f = strtof(number, NULL);
        if (errno || !isfinite(f)) return 0;
        p->dom->nodes[node].value.number = f;
    } else {
        long long value = strtoll(number, NULL, 10);
        if (errno || value < INT32_MIN || value > INT32_MAX) return 0;
        p->dom->nodes[node].value.integer = (int32_t)value;
    }
    return node;
}

static uint32_t catalog_json_parse(struct catalog_json *dom, const char *bytes, size_t length)
{
    uint32_t before_nodes = dom->node_count, before_strings = dom->string_size;
    struct catalog_json_parser p = {dom, bytes, bytes + length};
    errno = 0; uint32_t root = catalog_json_parse_value(&p, 0); catalog_json_space(&p);
    if (!root || p.cursor != p.end) {
        dom->node_count = before_nodes; dom->string_size = before_strings;
        if (!errno) errno = EINVAL;
        return 0;
    }
    return root;
}
#endif
